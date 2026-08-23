//! Central ownership and shutdown for every long-lived child process.
//!
//! Gateway startup is serialized independently from scheduler operations. Shutdown
//! takes the lifecycle locks in gateway-then-scheduler order, marks the manager as
//! stopping before it waits, and drains every owned handle exactly once.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const GATEWAY_READY_TIMEOUT: Duration = Duration::from_secs(15);
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct GatewayOwnership {
    pid: u32,
    executable: PathBuf,
}

struct ManagedChild {
    name: String,
    child: Child,
}

#[derive(Default)]
struct ProcessState {
    gateway: Option<Child>,
    scheduler: Option<Child>,
    agents: HashMap<String, Child>,
    agent_stdins: HashMap<String, Arc<Mutex<ChildStdin>>>,
}

pub(crate) struct ProcessManager {
    state: Mutex<ProcessState>,
    gateway_lifecycle: Mutex<()>,
    scheduler_lifecycle: Mutex<()>,
    // Not redundant with `shutting_down`: this makes a second shutdown() caller wait
    // until the children are actually dead, so RunEvent::Exit cannot return while the
    // signal handler is still killing.
    shutdown_lifecycle: Mutex<()>,
    shutting_down: AtomicBool,
    gateway_enabled: bool,
    ownership_path: PathBuf,
    readiness_timeout: Duration,
    shutdown_timeout: Duration,
}

impl ProcessManager {
    pub(crate) fn new() -> Self {
        Self::with_options(
            !crate::env::mock_mode(),
            crate::env::home_dir()
                .join(".infer")
                .join("run")
                .join("desktop-gateway.json"),
            GATEWAY_READY_TIMEOUT,
            GRACEFUL_SHUTDOWN_TIMEOUT,
        )
    }

    fn with_options(
        gateway_enabled: bool,
        ownership_path: PathBuf,
        readiness_timeout: Duration,
        shutdown_timeout: Duration,
    ) -> Self {
        Self {
            state: Mutex::new(ProcessState::default()),
            gateway_lifecycle: Mutex::new(()),
            scheduler_lifecycle: Mutex::new(()),
            shutdown_lifecycle: Mutex::new(()),
            shutting_down: AtomicBool::new(false),
            gateway_enabled,
            ownership_path,
            readiness_timeout,
            shutdown_timeout,
        }
    }

    pub(crate) fn start_gateway(&self, force: bool) -> Result<(), String> {
        if !self.gateway_enabled {
            return Ok(());
        }
        let _lifecycle = self
            .gateway_lifecycle
            .lock()
            .map_err(|e| format!("gateway lifecycle mutex poisoned: {e}"))?;
        self.ensure_running()?;

        if let Some(mut child) = self.take_gateway()? {
            let child_running = child
                .try_wait()
                .map_err(|e| format!("failed to inspect gateway process: {e}"))?
                .is_none();
            if child_running && !force && crate::gateway::gateway_reachable() {
                self.store_gateway(child)?;
                return Ok(());
            }
            stop_children(
                vec![ManagedChild {
                    name: "gateway".into(),
                    child,
                }],
                self.shutdown_timeout,
            )?;
            self.clear_stopped_gateway_ownership()?;
        }

        self.recover_owned_gateway(&crate::gateway::gateway_bin_path())?;
        self.ensure_running()?;

        if crate::gateway::gateway_reachable() {
            return Ok(());
        }

        let bin = crate::gateway::ensure_gateway_binary(force)?;
        self.ensure_running()?;
        let mut child = crate::gateway::spawn_gateway(&bin)?;

        let ownership = match ownership_for_child(&child) {
            Ok(ownership) => ownership,
            Err(error) => return Err(self.abort_start(child, error)),
        };
        if let Err(error) = self.write_ownership(&ownership) {
            return Err(self.abort_start(child, error));
        }
        if let Err(error) = wait_for_gateway_ready(
            &mut child,
            self.readiness_timeout,
            &self.shutting_down,
            crate::gateway::gateway_reachable,
        ) {
            return Err(self.abort_start(child, error));
        }
        if let Err(error) = self.ensure_running() {
            return Err(self.abort_start(child, error));
        }

        match self.store_gateway(child) {
            Ok(()) => Ok(()),
            Err(error) => Err(with_cleanup_errors(
                error,
                None,
                self.clear_stopped_gateway_ownership().err(),
            )),
        }
    }

    fn abort_start(&self, child: Child, error: String) -> String {
        let stop_error = stop_children(
            vec![ManagedChild {
                name: "gateway".into(),
                child,
            }],
            self.shutdown_timeout,
        )
        .err();
        with_cleanup_errors(
            error,
            stop_error,
            self.clear_stopped_gateway_ownership().err(),
        )
    }

    pub(crate) fn insert_agent(
        &self,
        session_id: String,
        mut child: Child,
        stdin: ChildStdin,
    ) -> Result<(), String> {
        if self.shutting_down.load(Ordering::SeqCst) {
            force_stop_child("agent", &mut child)?;
            return Err("application is shutting down".into());
        }

        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(error) => {
                let message = format!("process state mutex poisoned: {error}");
                drop(error);
                force_stop_child("agent", &mut child)?;
                return Err(message);
            }
        };
        if self.shutting_down.load(Ordering::SeqCst) {
            drop(state);
            force_stop_child("agent", &mut child)?;
            return Err("application is shutting down".into());
        }
        if state.agents.contains_key(&session_id) {
            drop(state);
            force_stop_child("duplicate agent", &mut child)?;
            return Err(format!("agent session {session_id} is already running"));
        }

        state
            .agent_stdins
            .insert(session_id.clone(), Arc::new(Mutex::new(stdin)));
        state.agents.insert(session_id, child);
        Ok(())
    }

    pub(crate) fn write_agent(&self, session_id: &str, bytes: &[u8]) -> Result<(), String> {
        let stdin = self
            .lock_state()?
            .agent_stdins
            .get(session_id)
            .cloned()
            .ok_or("No running agent")?;
        let mut stdin = stdin
            .lock()
            .map_err(|e| format!("agent stdin mutex poisoned: {e}"))?;
        stdin.write_all(bytes).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }

    pub(crate) fn remove_agent(&self, session_id: &str) -> Result<Option<Child>, String> {
        let mut state = self.lock_state()?;
        state.agent_stdins.remove(session_id);
        Ok(state.agents.remove(session_id))
    }

    pub(crate) fn cancel_agent(&self, session_id: &str) -> Result<(), String> {
        if let Some(mut child) = self.remove_agent(session_id)? {
            force_stop_child("agent", &mut child)?;
        }
        Ok(())
    }

    pub(crate) fn restart_scheduler<F>(&self, spawn: F) -> Result<(), String>
    where
        F: FnOnce() -> Result<Child, String>,
    {
        let _lifecycle = self
            .scheduler_lifecycle
            .lock()
            .map_err(|e| format!("scheduler lifecycle mutex poisoned: {e}"))?;
        self.ensure_running()?;

        if let Some(child) = self.take_scheduler()? {
            stop_children(
                vec![ManagedChild {
                    name: "scheduler".into(),
                    child,
                }],
                self.shutdown_timeout,
            )?;
        }

        let child = spawn()?;
        if let Err(error) = self.ensure_running() {
            let stop_error = stop_children(
                vec![ManagedChild {
                    name: "scheduler".into(),
                    child,
                }],
                self.shutdown_timeout,
            )
            .err();
            return Err(with_stop_error(error, stop_error));
        }

        self.store_scheduler(child)
    }

    pub(crate) fn stop_scheduler(&self) -> Result<(), String> {
        let _lifecycle = self
            .scheduler_lifecycle
            .lock()
            .map_err(|e| format!("scheduler lifecycle mutex poisoned: {e}"))?;
        if let Some(child) = self.take_scheduler()? {
            stop_children(
                vec![ManagedChild {
                    name: "scheduler".into(),
                    child,
                }],
                self.shutdown_timeout,
            )?;
        }
        Ok(())
    }

    pub(crate) fn scheduler_running(&self) -> Result<bool, String> {
        let mut state = self.lock_state()?;
        let Some(child) = state.scheduler.as_mut() else {
            return Ok(false);
        };
        match child.try_wait() {
            Ok(None) => Ok(true),
            Ok(Some(_)) => {
                state.scheduler = None;
                Ok(false)
            }
            Err(error) => Err(format!("failed to inspect scheduler process: {error}")),
        }
    }

    pub(crate) fn shutdown(&self) -> Result<(), String> {
        let _shutdown = lock_for_shutdown(&self.shutdown_lifecycle, "shutdown lifecycle");
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let _gateway = lock_for_shutdown(&self.gateway_lifecycle, "gateway lifecycle");
        let _scheduler = lock_for_shutdown(&self.scheduler_lifecycle, "scheduler lifecycle");

        let (gateway_owned, children, stdins) = {
            let mut state = lock_for_shutdown(&self.state, "process state");
            let gateway = state.gateway.take();
            let gateway_owned = gateway.is_some();
            let mut children = Vec::with_capacity(state.agents.len() + 2);
            if let Some(child) = gateway {
                children.push(ManagedChild {
                    name: "gateway".into(),
                    child,
                });
            }
            if let Some(child) = state.scheduler.take() {
                children.push(ManagedChild {
                    name: "scheduler".into(),
                    child,
                });
            }
            children.extend(
                state
                    .agents
                    .drain()
                    .map(|(session_id, child)| ManagedChild {
                        name: format!("agent {session_id}"),
                        child,
                    }),
            );
            let stdins = state
                .agent_stdins
                .drain()
                .map(|(_, stdin)| stdin)
                .collect::<Vec<_>>();
            (gateway_owned, children, stdins)
        };

        drop(stdins);
        let stop_error = stop_children(children, self.shutdown_timeout).err();
        let ownership_error = if gateway_owned {
            self.clear_stopped_gateway_ownership().err()
        } else if self.gateway_enabled {
            self.recover_owned_gateway(&crate::gateway::gateway_bin_path())
                .err()
        } else {
            None
        };

        match (stop_error, ownership_error) {
            (None, None) => Ok(()),
            (Some(error), None) | (None, Some(error)) => Err(error),
            (Some(stop), Some(ownership)) => Err(format!("{stop}; {ownership}")),
        }
    }

    fn ensure_running(&self) -> Result<(), String> {
        if self.shutting_down.load(Ordering::SeqCst) {
            Err("application is shutting down".into())
        } else {
            Ok(())
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, ProcessState>, String> {
        self.state
            .lock()
            .map_err(|e| format!("process state mutex poisoned: {e}"))
    }

    fn take_gateway(&self) -> Result<Option<Child>, String> {
        Ok(self.lock_state()?.gateway.take())
    }

    fn store_gateway(&self, child: Child) -> Result<(), String> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(error) => {
                let message = format!("process state mutex poisoned: {error}");
                drop(error);
                let stop_error = stop_children(
                    vec![ManagedChild {
                        name: "gateway".into(),
                        child,
                    }],
                    self.shutdown_timeout,
                )
                .err();
                return Err(with_stop_error(message, stop_error));
            }
        };
        if self.shutting_down.load(Ordering::SeqCst) {
            drop(state);
            let stop_error = stop_children(
                vec![ManagedChild {
                    name: "gateway".into(),
                    child,
                }],
                self.shutdown_timeout,
            )
            .err();
            return Err(with_stop_error(
                "application is shutting down".into(),
                stop_error,
            ));
        }
        state.gateway = Some(child);
        Ok(())
    }

    fn take_scheduler(&self) -> Result<Option<Child>, String> {
        Ok(self.lock_state()?.scheduler.take())
    }

    fn store_scheduler(&self, child: Child) -> Result<(), String> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(error) => {
                let message = format!("process state mutex poisoned: {error}");
                drop(error);
                let stop_error = stop_children(
                    vec![ManagedChild {
                        name: "scheduler".into(),
                        child,
                    }],
                    self.shutdown_timeout,
                )
                .err();
                return Err(with_stop_error(message, stop_error));
            }
        };
        if self.shutting_down.load(Ordering::SeqCst) {
            drop(state);
            let stop_error = stop_children(
                vec![ManagedChild {
                    name: "scheduler".into(),
                    child,
                }],
                self.shutdown_timeout,
            )
            .err();
            return Err(with_stop_error(
                "application is shutting down".into(),
                stop_error,
            ));
        }
        state.scheduler = Some(child);
        Ok(())
    }

    fn recover_owned_gateway(&self, expected_executable: &Path) -> Result<(), String> {
        let Some(ownership) = self.read_ownership()? else {
            return Ok(());
        };
        if !same_executable(&ownership.executable, expected_executable) {
            self.remove_ownership()?;
            return Ok(());
        }

        if !process_matches(&ownership)? {
            return self.remove_ownership();
        }

        terminate_owned_pid(&ownership, self.shutdown_timeout)?;
        self.remove_ownership()
    }

    fn clear_stopped_gateway_ownership(&self) -> Result<(), String> {
        let Some(ownership) = self.read_ownership()? else {
            return Ok(());
        };
        if process_matches(&ownership)? {
            return Err(format!(
                "gateway process {} is still running; ownership metadata retained at {}",
                ownership.pid,
                self.ownership_path.display()
            ));
        }
        self.remove_ownership()
    }

    fn read_ownership(&self) -> Result<Option<GatewayOwnership>, String> {
        let contents = match std::fs::read_to_string(&self.ownership_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "failed to read gateway ownership {}: {error}",
                    self.ownership_path.display()
                ));
            }
        };
        match serde_json::from_str(&contents) {
            Ok(ownership) => Ok(Some(ownership)),
            Err(error) => {
                self.remove_ownership()?;
                eprintln!(
                    "removed invalid gateway ownership metadata {}: {error}",
                    self.ownership_path.display()
                );
                Ok(None)
            }
        }
    }

    fn write_ownership(&self, ownership: &GatewayOwnership) -> Result<(), String> {
        let parent = self
            .ownership_path
            .parent()
            .ok_or_else(|| "gateway ownership path has no parent".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create gateway ownership directory {}: {error}",
                parent.display()
            )
        })?;
        let temporary = self.ownership_path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(ownership)
            .map_err(|error| format!("failed to serialize gateway ownership: {error}"))?;
        std::fs::write(&temporary, bytes).map_err(|error| {
            format!(
                "failed to write gateway ownership {}: {error}",
                temporary.display()
            )
        })?;
        std::fs::rename(&temporary, &self.ownership_path).map_err(|error| {
            format!(
                "failed to publish gateway ownership {}: {error}",
                self.ownership_path.display()
            )
        })
    }

    fn remove_ownership(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.ownership_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "failed to remove gateway ownership {}: {error}",
                self.ownership_path.display()
            )),
        }
    }
}

fn ownership_for_child(child: &Child) -> Result<GatewayOwnership, String> {
    let pid = child.id();
    let executable = process_executable(pid)?
        .ok_or_else(|| format!("gateway process {pid} exited before ownership was recorded"))?;
    Ok(GatewayOwnership { pid, executable })
}

fn wait_for_gateway_ready<F>(
    child: &mut Child,
    timeout: Duration,
    shutting_down: &AtomicBool,
    mut reachable: F,
) -> Result<(), String>
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        if shutting_down.load(Ordering::SeqCst) {
            return Err("application is shutting down".into());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to inspect starting gateway: {error}"))?
        {
            return Err(format!("gateway exited before becoming ready: {status}"));
        }
        if reachable() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "gateway did not become ready within {} seconds",
                timeout.as_secs_f32()
            ));
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL.min(timeout));
    }
}

fn stop_children(mut children: Vec<ManagedChild>, timeout: Duration) -> Result<(), String> {
    let mut errors = Vec::new();
    for process in &mut children {
        match process.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = signal_terminate(&mut process.child) {
                    errors.push(format!("failed to terminate {}: {error}", process.name));
                }
            }
            Err(error) => errors.push(format!("failed to inspect {}: {error}", process.name)),
        }
    }

    let deadline = Instant::now() + timeout;
    while !children.is_empty() && Instant::now() < deadline {
        let mut index = 0;
        while index < children.len() {
            match children[index].child.try_wait() {
                Ok(Some(_)) => {
                    children.swap_remove(index);
                }
                Ok(None) => index += 1,
                Err(error) => {
                    errors.push(format!(
                        "failed to wait for {}: {error}",
                        children[index].name
                    ));
                    index += 1;
                }
            }
        }
        if !children.is_empty() {
            std::thread::sleep(PROCESS_POLL_INTERVAL.min(timeout));
        }
    }

    for mut process in children {
        if let Err(error) = process.child.kill() {
            errors.push(format!("failed to force-kill {}: {error}", process.name));
        }
        if let Err(error) = process.child.wait() {
            errors.push(format!("failed to reap {}: {error}", process.name));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(unix)]
fn signal_terminate(child: &mut Child) -> Result<(), String> {
    send_unix_signal(child.id(), nix::sys::signal::Signal::SIGTERM)
}

#[cfg(not(unix))]
fn signal_terminate(child: &mut Child) -> Result<(), String> {
    child.kill().map_err(|error| error.to_string())
}

#[cfg(unix)]
fn send_unix_signal(pid: u32, signal: nix::sys::signal::Signal) -> Result<(), String> {
    let pid = i32::try_from(pid).map_err(|_| format!("process id {pid} exceeds i32"))?;
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn force_stop_child(name: &str, child: &mut Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|error| format!("failed to inspect {name}: {error}"))?
        .is_some()
    {
        return Ok(());
    }
    child
        .kill()
        .map_err(|error| format!("failed to force-kill {name}: {error}"))?;
    child
        .wait()
        .map_err(|error| format!("failed to reap {name}: {error}"))?;
    Ok(())
}

fn lock_for_shutdown<'a, T>(mutex: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        eprintln!("recovering poisoned {name} mutex during shutdown");
        poisoned.into_inner()
    })
}

#[cfg(unix)]
fn terminate_owned_pid(ownership: &GatewayOwnership, timeout: Duration) -> Result<(), String> {
    send_unix_signal(ownership.pid, nix::sys::signal::Signal::SIGTERM)
        .map_err(|error| format!("failed to terminate recovered gateway: {error}"))?;
    if wait_for_owned_pid_exit(ownership.pid, timeout)? {
        return Ok(());
    }
    send_unix_signal(ownership.pid, nix::sys::signal::Signal::SIGKILL)
        .map_err(|error| format!("failed to force-kill recovered gateway: {error}"))?;
    if wait_for_owned_pid_exit(ownership.pid, Duration::from_secs(1))? {
        Ok(())
    } else {
        Err(format!(
            "recovered gateway process {} remained alive after SIGKILL",
            ownership.pid
        ))
    }
}

#[cfg(not(unix))]
fn terminate_owned_pid(ownership: &GatewayOwnership, _timeout: Duration) -> Result<(), String> {
    Err(format!(
        "cannot recover gateway process {} on this platform",
        ownership.pid
    ))
}

// ponytail: the executable path alone identifies the process. A recycled pid would only
// be mistaken for the gateway if it is also running the gateway binary - which is exactly
// what we want to terminate anyway - so the start time is not worth a second lookup.
#[cfg(unix)]
fn wait_for_owned_pid_exit(pid: u32, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if !process_exists(pid)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL.min(timeout));
    }
}

fn process_matches(ownership: &GatewayOwnership) -> Result<bool, String> {
    Ok(process_executable(ownership.pid)?
        .is_some_and(|executable| executable == ownership.executable))
}

#[cfg(target_os = "linux")]
fn process_executable(pid: u32) -> Result<Option<PathBuf>, String> {
    match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "failed to inspect process {pid} executable: {error}"
        )),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_executable(pid: u32) -> Result<Option<PathBuf>, String> {
    Ok(ps_field(pid, "comm")?.map(PathBuf::from))
}

#[cfg(not(unix))]
fn process_executable(_pid: u32) -> Result<Option<PathBuf>, String> {
    Ok(None)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn ps_field(pid: u32, field: &str) -> Result<Option<String>, String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", &format!("{field}=")])
        .output()
        .map_err(|error| format!("failed to inspect process {pid}: {error}"))?;
    if !output.status.success() {
        return if process_exists(pid)? {
            Err(format!(
                "ps could not inspect running process {pid}: {}",
                output.status
            ))
        } else {
            Ok(None)
        };
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(unix)]
fn process_exists(pid: u32) -> Result<bool, String> {
    let pid = i32::try_from(pid).map_err(|_| format!("process id {pid} exceeds i32"))?;
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
        Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(true),
        Err(nix::errno::Errno::ESRCH) => Ok(false),
        Err(error) => Err(format!("failed to inspect process existence: {error}")),
    }
}

fn same_executable(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn with_stop_error(error: String, stop_error: Option<String>) -> String {
    match stop_error {
        Some(stop) => format!("{error}; cleanup failed: {stop}"),
        None => error,
    }
}

fn with_cleanup_errors(
    error: String,
    stop_error: Option<String>,
    ownership_error: Option<String>,
) -> String {
    let error = with_stop_error(error, stop_error);
    match ownership_error {
        Some(ownership) => format!("{error}; ownership cleanup failed: {ownership}"),
        None => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier};

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_manager(name: &str) -> ProcessManager {
        let path = std::env::temp_dir().join(format!(
            "desktop-process-manager-{name}-{}-{}.json",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        ProcessManager::with_options(
            true,
            path,
            Duration::from_secs(1),
            Duration::from_millis(250),
        )
    }

    #[cfg(unix)]
    fn spawn_signal_child(trap: &str) -> Child {
        let mut child = Command::new("sh")
            .args([
                "-c",
                &format!("trap '{trap}' TERM; echo ready; while :; do sleep 0.01; done"),
            ])
            .stdout(Stdio::piped())
            .spawn()
            .expect("test signal child should spawn");
        let mut ready = String::new();
        BufReader::new(
            child
                .stdout
                .take()
                .expect("test signal child stdout should be piped"),
        )
        .read_line(&mut ready)
        .expect("test signal child should report readiness");
        assert_eq!(ready.trim(), "ready");
        child
    }

    #[test]
    fn ownership_metadata_round_trips() {
        let manager = test_manager("round-trip");
        let ownership = GatewayOwnership {
            pid: 42,
            executable: PathBuf::from("/tmp/gateway"),
        };

        manager
            .write_ownership(&ownership)
            .expect("ownership should be written");

        assert_eq!(
            manager.read_ownership().expect("ownership should be read"),
            Some(ownership)
        );
        manager
            .remove_ownership()
            .expect("ownership should be removed");
    }

    #[test]
    fn invalid_ownership_metadata_is_removed() {
        let manager = test_manager("invalid");
        let parent = manager
            .ownership_path
            .parent()
            .expect("test path should have a parent");
        std::fs::create_dir_all(parent).expect("test directory should be created");
        std::fs::write(&manager.ownership_path, b"not-json")
            .expect("invalid ownership should be written");

        assert_eq!(
            manager
                .read_ownership()
                .expect("invalid metadata is handled"),
            None
        );
        assert!(!manager.ownership_path.exists());
    }

    #[test]
    fn gateway_lifecycle_is_serialized() {
        let manager = Arc::new(test_manager("serialized"));
        let barrier = Arc::new(Barrier::new(2));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first_manager = Arc::clone(&manager);
        let first_barrier = Arc::clone(&barrier);
        let first = std::thread::spawn(move || {
            let _guard = first_manager
                .gateway_lifecycle
                .lock()
                .expect("gateway lifecycle should lock");
            first_barrier.wait();
            release_rx
                .recv()
                .expect("first operation should be released");
        });
        barrier.wait();

        let second_manager = Arc::clone(&manager);
        let second = std::thread::spawn(move || {
            let _guard = second_manager
                .gateway_lifecycle
                .lock()
                .expect("gateway lifecycle should lock after first");
            entered_tx
                .send(())
                .expect("second operation should report entry");
        });

        assert!(entered_rx.recv_timeout(Duration::from_millis(50)).is_err());
        release_tx
            .send(())
            .expect("first operation should be released");
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second operation should enter after release");
        first.join().expect("first operation should finish");
        second.join().expect("second operation should finish");
    }

    #[cfg(unix)]
    #[test]
    fn recovered_gateway_gets_sigterm_and_metadata_is_removed() {
        let manager = Arc::new(test_manager("recover"));
        let mut child = spawn_signal_child("exit 0");
        let ownership = ownership_for_child(&child).expect("child identity should be available");
        let executable = ownership.executable.clone();
        manager
            .write_ownership(&ownership)
            .expect("ownership should be written");

        let recovery_manager = Arc::clone(&manager);
        let recovery =
            std::thread::spawn(move || recovery_manager.recover_owned_gateway(&executable));
        let status = child.wait().expect("recovered child should be reaped");

        assert!(status.success());
        recovery
            .join()
            .expect("recovery thread should finish")
            .expect("recovery should succeed");
        assert!(!manager.ownership_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn ignored_sigterm_escalates_to_sigkill_within_bound() {
        let child = spawn_signal_child("");
        let pid = child.id();
        let started = Instant::now();

        stop_children(
            vec![ManagedChild {
                name: "stubborn child".into(),
                child,
            }],
            Duration::from_millis(100),
        )
        .expect("forced shutdown should succeed");

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            process_executable(pid)
                .expect("process lookup should succeed")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_is_idempotent() {
        let manager = test_manager("idempotent");
        let child = spawn_signal_child("exit 0");
        let ownership = ownership_for_child(&child).expect("child identity should be available");
        let pid = ownership.pid;
        manager
            .write_ownership(&ownership)
            .expect("ownership should be written");
        manager
            .store_gateway(child)
            .expect("gateway child should be stored");

        manager.shutdown().expect("first shutdown should succeed");
        manager
            .shutdown()
            .expect("second shutdown should be a no-op");

        assert!(
            process_executable(pid)
                .expect("process lookup should succeed")
                .is_none()
        );
        assert!(!manager.ownership_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn agent_insert_is_rejected_during_shutdown() {
        let manager = test_manager("insert-during-shutdown");
        manager.shutdown().expect("shutdown should succeed");
        let mut child = Command::new("sh")
            .args(["-c", "while :; do sleep 0.01; done"])
            .stdin(Stdio::piped())
            .spawn()
            .expect("test child should spawn");
        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .expect("test child stdin should be piped");

        let error = manager
            .insert_agent("session".into(), child, stdin)
            .expect_err("agent insert should be rejected during shutdown");

        assert_eq!(error, "application is shutting down");
        assert!(
            process_executable(pid)
                .expect("process lookup should succeed")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn readiness_wait_observes_health_after_retries() {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 5"])
            .spawn()
            .expect("test child should spawn");
        let attempts = std::cell::Cell::new(0);

        wait_for_gateway_ready(
            &mut child,
            Duration::from_secs(1),
            &AtomicBool::new(false),
            || {
                attempts.set(attempts.get() + 1);
                attempts.get() == 3
            },
        )
        .expect("third health check should be ready");
        force_stop_child("test child", &mut child).expect("test child should stop");

        assert_eq!(attempts.get(), 3);
    }
}
