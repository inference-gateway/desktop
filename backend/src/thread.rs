//! One WebSocket connection per chat to the daemon's AG-UI binding: the thread
//! the connection follows, the frames the app sends on it, and the AG-UI
//! events the parser turns the daemon's frames into. Each socket is owned by
//! one thread running `pump`, which polls with a short read timeout and
//! drains an mpsc queue of outbound frames, so no lock spans socket I/O.

use crate::agent::{AgentEvent, AgentParser};
use crate::daemon::{Binding, ExtensionStatus, PROTOCOL_VERSION, lock};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// A worker launch starts the project's gateway and MCP servers before it
/// answers with the history, so the first snapshot takes its time.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_millis(50);
const PING_EVERY: Duration = Duration::from_secs(20);

/// What a thread's worker launches with. The daemon applies these once, when
/// the worker starts, so a thread reopened later keeps its first options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ThreadOptions {
    pub(crate) model: String,
    pub(crate) mode: String,
    pub(crate) system_prompt: Option<String>,
    pub(crate) custom_instructions: Option<String>,
    pub(crate) sandbox_directories: Vec<String>,
    pub(crate) max_turns: u32,
}

pub(crate) type Sink = Arc<dyn Fn(AgentEvent) + Send + Sync>;
pub(crate) type StatusSink = Arc<dyn Fn(ExtensionStatus) + Send + Sync>;

/// A connection following one thread. Dropping the sender ends the pump.
pub(crate) struct Thread {
    session_id: String,
    tx: mpsc::Sender<String>,
    history: Vec<serde_json::Value>,
    model: Mutex<String>,
    mode: Mutex<String>,
    parser: Mutex<AgentParser>,
    open_interrupts: Mutex<Vec<String>>,
    answers: Mutex<Vec<serde_json::Value>>,
    sink: Sink,
    status: StatusSink,
    pub(crate) on_done: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl Thread {
    /// Dials the binding, presents the hello, and follows the conversation
    /// `session_id` in `project_dir` (a missing one is created by the worker).
    /// Returns once the worker answered with the thread's history.
    pub(crate) fn open(
        binding: &Binding,
        session_id: &str,
        project_dir: &str,
        options: &ThreadOptions,
        sink: Sink,
        status: StatusSink,
    ) -> Result<Arc<Thread>, String> {
        let stream = TcpStream::connect(("127.0.0.1", binding.port)).map_err(|e| {
            format!(
                "infer daemon is not listening on port {}: {e}",
                binding.port
            )
        })?;
        stream
            .set_read_timeout(Some(HELLO_TIMEOUT))
            .map_err(|e| e.to_string())?;
        let (mut ws, _) =
            tungstenite::client(format!("ws://127.0.0.1:{}/ws", binding.port), stream)
                .map_err(|e| format!("infer daemon refused the connection: {e}"))?;
        ws.send(Message::text(hello_frame(&binding.token)))
            .map_err(|e| e.to_string())?;
        let ack = read_text(&mut ws)?;
        check_ack(&ack)?;
        ws.send(Message::text(open_frame(session_id, project_dir, options)))
            .map_err(|e| e.to_string())?;

        let _ = ws.get_ref().set_read_timeout(Some(SNAPSHOT_TIMEOUT));
        let history = loop {
            let text = read_text(&mut ws)?;
            let frame: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            match frame.get("type").and_then(|t| t.as_str()) {
                Some("MESSAGES_SNAPSHOT") => {
                    break frame
                        .get("messages")
                        .and_then(|m| m.as_array())
                        .cloned()
                        .unwrap_or_default();
                }
                Some("browser_extension_status") => status(extension_status(&frame)),
                Some("RUN_ERROR") => {
                    return Err(frame
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("the daemon could not open the thread")
                        .to_string());
                }
                _ => {}
            }
        };

        let (tx, rx) = mpsc::channel();
        let thread = Arc::new(Thread {
            session_id: session_id.to_string(),
            tx,
            history,
            model: Mutex::new(options.model.clone()),
            mode: Mutex::new(options.mode.clone()),
            parser: Mutex::new(AgentParser::new(Some(session_id.to_string()))),
            open_interrupts: Mutex::default(),
            answers: Mutex::default(),
            sink,
            status,
            on_done: Mutex::new(None),
        });
        let follower = Arc::clone(&thread);
        std::thread::spawn(move || pump(ws, rx, |text| follower.handle(text)));
        Ok(thread)
    }

    /// The messages the worker restored for this thread when it was opened.
    pub(crate) fn history(&self) -> &[serde_json::Value] {
        &self.history
    }

    fn send(&self, frame: serde_json::Value) -> Result<(), String> {
        self.tx
            .send(frame.to_string())
            .map_err(|_| format!("the connection for chat {} is closed", self.session_id))
    }

    /// Switches the thread's model and mode for its next turns when they
    /// differ from what the worker runs with.
    pub(crate) fn select(&self, model: &str, mode: &str) -> Result<(), String> {
        if !model.is_empty() && *lock(&self.model) != model {
            self.send(serde_json::json!({ "type": "select_model", "model": model }))?;
            *lock(&self.model) = model.to_string();
        }
        if !mode.is_empty() && *lock(&self.mode) != mode {
            self.send(serde_json::json!({ "type": "set_mode", "mode": mode }))?;
            *lock(&self.mode) = mode.to_string();
        }
        Ok(())
    }

    /// Starts the thread's next run with one user message, or continues it
    /// after a stop when `content` is None.
    pub(crate) fn run(&self, content: Option<serde_json::Value>) -> Result<(), String> {
        let messages = match content {
            Some(content) => vec![serde_json::json!({
                "id": uuid(),
                "role": "user",
                "content": content,
            })],
            None => Vec::new(),
        };
        self.send(serde_json::json!({
            "type": "run_agent_input",
            "input": { "threadId": self.session_id, "runId": uuid(), "messages": messages },
        }))
    }

    pub(crate) fn interrupt(&self) -> Result<(), String> {
        self.send(serde_json::json!({ "type": "interrupt" }))
    }

    /// Answers one interrupt of the suspended run. The resume goes out once
    /// every open interrupt has an answer, in one run_agent_input.
    pub(crate) fn resume(&self, entry: serde_json::Value) -> Result<(), String> {
        let id = entry
            .get("interruptId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let open = lock(&self.open_interrupts);
        if !open.contains(&id) {
            return Err(format!(
                "no open interrupt {id} on chat {}",
                self.session_id
            ));
        }
        let mut answers = lock(&self.answers);
        answers.retain(|a| a.get("interruptId").and_then(|v| v.as_str()) != Some(&id));
        answers.push(entry);
        let answered = open.iter().all(|o| {
            answers
                .iter()
                .any(|a| a.get("interruptId").and_then(|v| v.as_str()) == Some(o))
        });
        if !answered {
            return Ok(());
        }
        let resume: Vec<serde_json::Value> = answers.drain(..).collect();
        drop(answers);
        drop(open);
        lock(&self.open_interrupts).clear();
        self.send(serde_json::json!({
            "type": "run_agent_input",
            "input": { "threadId": self.session_id, "runId": uuid(), "messages": [], "resume": resume },
        }))
    }

    /// Folds one frame of the thread into agent events: the extension's state
    /// stays an app frame, everything uppercase goes through the parser, and
    /// a run's terminal event is followed by the Done the UI keys busy on.
    fn handle(&self, text: &str) {
        let frame: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };
        let Some(kind) = frame.get("type").and_then(|t| t.as_str()) else {
            return;
        };
        if kind == "browser_extension_status" {
            (self.status)(extension_status(&frame));
            return;
        }
        if kind != kind.to_uppercase() {
            return;
        }
        if kind == "RUN_STARTED" {
            lock(&self.open_interrupts).clear();
            lock(&self.answers).clear();
        }
        if let Some(event) = lock(&self.parser).parse_line(text) {
            (self.sink)(event);
        }
        match kind {
            "RUN_FINISHED" => {
                let outcome = frame
                    .get("outcome")
                    .and_then(|o| o.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("success");
                match outcome {
                    "interrupt" => {
                        let ids = frame
                            .get("outcome")
                            .and_then(|o| o.get("interrupts"))
                            .and_then(|i| i.as_array())
                            .map(|list| {
                                list.iter()
                                    .filter_map(|i| i.get("id").and_then(|v| v.as_str()))
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        *lock(&self.open_interrupts) = ids;
                    }
                    "cancelled" => {
                        (self.sink)(AgentEvent::Cancelled);
                        self.done(0);
                    }
                    _ => self.done(0),
                }
            }
            "RUN_ERROR" if frame.get("runId").and_then(|r| r.as_str()).is_some() => {
                self.done(1);
            }
            _ => {}
        }
    }

    fn done(&self, exit_code: i32) {
        if let Some(on_done) = lock(&self.on_done).as_ref() {
            on_done();
        }
        (self.sink)(AgentEvent::Done {
            exit_code,
            stderr: String::new(),
        });
    }
}

fn uuid() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

fn hello_frame(token: &str) -> String {
    serde_json::json!({
        "type": "browser_hello",
        "token": token,
        "client": "desktop",
        "protocol_version": PROTOCOL_VERSION,
        "extension_version": env!("CARGO_PKG_VERSION"),
    })
    .to_string()
}

/// The daemon accepts any valid hello, but a binding of another protocol
/// version is one the app cannot drive: say so instead of a silent panel.
fn check_ack(text: &str) -> Result<(), String> {
    let ack: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if ack.get("type").and_then(|t| t.as_str()) != Some("browser_hello_ack") {
        return Err("infer daemon rejected the hello (check the binding token)".into());
    }
    match ack.get("protocol_version").and_then(|v| v.as_u64()) {
        Some(v) if v == PROTOCOL_VERSION => Ok(()),
        other => Err(format!(
            "infer daemon speaks protocol version {}, this app needs {PROTOCOL_VERSION}: update infer",
            other.map_or("unknown".to_string(), |v| v.to_string())
        )),
    }
}

fn open_frame(session_id: &str, project_dir: &str, options: &ThreadOptions) -> String {
    let mut frame = serde_json::json!({
        "type": "resume_conversation",
        "project_dir": project_dir,
        "id": session_id,
        "max_turns": options.max_turns,
    });
    let set = |frame: &mut serde_json::Value, key: &str, value: Option<serde_json::Value>| {
        if let Some(value) = value {
            frame[key] = value;
        }
    };
    set(
        &mut frame,
        "model",
        (!options.model.is_empty()).then(|| options.model.clone().into()),
    );
    set(
        &mut frame,
        "mode",
        (!options.mode.is_empty()).then(|| options.mode.clone().into()),
    );
    set(
        &mut frame,
        "system_prompt",
        options.system_prompt.clone().map(Into::into),
    );
    set(
        &mut frame,
        "custom_instructions",
        options.custom_instructions.clone().map(Into::into),
    );
    if !options.sandbox_directories.is_empty() {
        frame["sandbox_directories"] = options.sandbox_directories.clone().into();
    }
    frame.to_string()
}

fn extension_status(frame: &serde_json::Value) -> ExtensionStatus {
    ExtensionStatus {
        connected: frame
            .get("connected")
            .and_then(|c| c.as_bool())
            .unwrap_or(false),
        version: frame
            .get("extension_version")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    }
}

fn read_text(ws: &mut WebSocket<TcpStream>) -> Result<String, String> {
    loop {
        match ws
            .read()
            .map_err(|e| format!("infer daemon connection failed: {e}"))?
        {
            Message::Text(text) => return Ok(text.to_string()),
            Message::Close(_) => return Err("infer daemon closed the connection".into()),
            _ => {}
        }
    }
}

/// Own `ws` until it closes: deliver inbound text frames to `on_frame`, write
/// queued outbound frames, ping every PING_EVERY.
fn pump(mut ws: WebSocket<TcpStream>, rx: mpsc::Receiver<String>, mut on_frame: impl FnMut(&str)) {
    let _ = ws.get_ref().set_read_timeout(Some(POLL));
    let mut last_ping = Instant::now();
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => on_frame(&text),
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
        loop {
            match rx.try_recv() {
                Ok(frame) => {
                    if ws.send(Message::text(frame)).is_err() {
                        return;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if last_ping.elapsed() >= PING_EVERY {
            if ws.send(Message::Ping(Vec::new().into())).is_err() {
                return;
            }
            last_ping = Instant::now();
        }
    }
}

/// The app's open threads by chat id.
#[derive(Default)]
pub(crate) struct Threads(Mutex<HashMap<String, Arc<Thread>>>);

impl Threads {
    pub(crate) fn get(&self, session_id: &str) -> Option<Arc<Thread>> {
        lock(&self.0).get(session_id).cloned()
    }

    pub(crate) fn insert(&self, session_id: &str, thread: Arc<Thread>) {
        lock(&self.0).insert(session_id.to_string(), thread);
    }

    pub(crate) fn remove(&self, session_id: &str) -> Option<Arc<Thread>> {
        lock(&self.0).remove(session_id)
    }
}

pub(crate) fn close_all(state: &crate::AppState) {
    lock(&state.threads.0).clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A daemon stand-in: accepts one client, answers the hello with `ack`,
    /// records every frame it receives, and plays `script` after the open.
    fn fake_daemon(ack: &str, script: Vec<&str>) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&received);
        let ack = ack.to_string();
        let script: Vec<String> = script.into_iter().map(str::to_owned).collect();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let hello = ws.read().unwrap();
            seen.lock().unwrap().push(hello.to_string());
            ws.send(Message::text(ack)).unwrap();
            let open = ws.read().unwrap();
            seen.lock().unwrap().push(open.to_string());
            for frame in script {
                ws.send(Message::text(frame)).unwrap();
            }
            while let Ok(msg) = ws.read() {
                match msg {
                    Message::Text(text) => seen.lock().unwrap().push(text.to_string()),
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        (port, received)
    }

    fn sinks() -> (
        Sink,
        Arc<Mutex<Vec<AgentEvent>>>,
        StatusSink,
        Arc<Mutex<Vec<ExtensionStatus>>>,
    ) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let e = Arc::clone(&events);
        let s = Arc::clone(&statuses);
        (
            Arc::new(move |ev: AgentEvent| e.lock().unwrap().push(ev)),
            events,
            Arc::new(move |st: ExtensionStatus| s.lock().unwrap().push(st)),
            statuses,
        )
    }

    fn wait_for<T>(items: &Mutex<Vec<T>>, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while items.lock().unwrap().len() < count {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {count} items"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn open_presents_the_hello_and_thread_options_and_reads_the_snapshot() {
        let (port, received) = fake_daemon(
            r#"{"type":"browser_hello_ack","protocol_version":2}"#,
            vec![
                r#"{"type":"browser_extension_status","connected":true,"extension_version":"1.9.2","protocol_version":1}"#,
                r#"{"type":"MESSAGES_SNAPSHOT","messages":[{"id":"m1","role":"user","content":"hi"}]}"#,
            ],
        );
        let (sink, _, status, statuses) = sinks();
        let options = ThreadOptions {
            model: "openai/gpt-4o".into(),
            mode: "auto".into(),
            system_prompt: Some("be brief".into()),
            custom_instructions: None,
            sandbox_directories: vec![".".into(), "/tmp".into()],
            max_turns: 500,
        };
        let binding = Binding {
            port,
            token: "tok".into(),
        };
        let thread = Thread::open(&binding, "s1", "/proj", &options, sink, status).unwrap();
        assert_eq!(thread.history().len(), 1);
        assert_eq!(
            statuses.lock().unwrap()[0],
            ExtensionStatus {
                connected: true,
                version: Some("1.9.2".into())
            }
        );
        let frames = received.lock().unwrap();
        let hello: serde_json::Value = serde_json::from_str(&frames[0]).unwrap();
        assert_eq!(hello["client"], "desktop");
        assert_eq!(hello["protocol_version"], 2);
        assert_eq!(hello["token"], "tok");
        let open: serde_json::Value = serde_json::from_str(&frames[1]).unwrap();
        assert_eq!(open["type"], "resume_conversation");
        assert_eq!(open["project_dir"], "/proj");
        assert_eq!(open["id"], "s1");
        assert_eq!(open["model"], "openai/gpt-4o");
        assert_eq!(open["mode"], "auto");
        assert_eq!(open["system_prompt"], "be brief");
        assert!(open.get("custom_instructions").is_none());
        assert_eq!(
            open["sandbox_directories"],
            serde_json::json!([".", "/tmp"])
        );
        assert_eq!(open["max_turns"], 500);
    }

    #[test]
    fn open_refuses_another_protocol_version() {
        let (port, _) = fake_daemon(
            r#"{"type":"browser_hello_ack","protocol_version":1}"#,
            vec![],
        );
        let (sink, _, status, _) = sinks();
        let binding = Binding {
            port,
            token: "tok".into(),
        };
        let err = Thread::open(
            &binding,
            "s1",
            "/proj",
            &ThreadOptions::default(),
            sink,
            status,
        )
        .err()
        .expect("another protocol version is refused");
        assert!(err.contains("update infer"), "{err}");
    }

    #[test]
    fn a_run_streams_events_and_an_interrupt_is_answered_by_one_resume() {
        let (port, received) = fake_daemon(
            r#"{"type":"browser_hello_ack","protocol_version":2}"#,
            vec![
                r#"{"type":"MESSAGES_SNAPSHOT","messages":[]}"#,
                r#"{"type":"RUN_STARTED","threadId":"s1","runId":"r1"}"#,
                r#"{"type":"TOOL_CALL_START","toolCallId":"c1","toolCallName":"Bash"}"#,
                r#"{"type":"TOOL_CALL_ARGS","toolCallId":"c1","delta":"{\"command\":\"ls\"}"}"#,
                r#"{"type":"TOOL_CALL_END","toolCallId":"c1"}"#,
                r#"{"type":"RUN_FINISHED","threadId":"s1","runId":"r1","outcome":{"type":"interrupt","interrupts":[{"id":"c1","reason":"tool_call","toolCallId":"c1"}]}}"#,
                r#"{"type":"RUN_STARTED","threadId":"s1","runId":"r2"}"#,
                r#"{"type":"RUN_FINISHED","threadId":"s1","runId":"r2","outcome":{"type":"cancelled"}}"#,
                r#"{"type":"RUN_ERROR","message":"no thread"}"#,
            ],
        );
        let (sink, events, status, _) = sinks();
        let binding = Binding {
            port,
            token: "tok".into(),
        };
        let thread = Thread::open(
            &binding,
            "s1",
            "/proj",
            &ThreadOptions::default(),
            sink,
            status,
        )
        .unwrap();
        thread.run(Some("hello".into())).unwrap();
        wait_for(&events, 8);
        let seen = events.lock().unwrap();
        assert!(
            matches!(&seen[1], AgentEvent::AssistantMessage { tool_calls, .. } if tool_calls[0].name == "Bash")
        );
        assert!(
            matches!(&seen[2], AgentEvent::ApprovalRequest { tool_call_id, tool_name, tool_args }
            if tool_call_id == "c1" && tool_name == "Bash" && tool_args == "{\"command\":\"ls\"}")
        );
        assert!(matches!(&seen[4], AgentEvent::TokenUsage { .. }));
        assert!(matches!(&seen[5], AgentEvent::Cancelled));
        assert!(matches!(&seen[6], AgentEvent::Done { exit_code: 0, .. }));
        assert_eq!(seen.len(), 8, "a RUN_ERROR without a runId ends no run");
        assert!(matches!(&seen[7], AgentEvent::AgentError { message } if message == "no thread"));
        drop(seen);

        assert!(
            thread
                .resume(serde_json::json!({ "interruptId": "zzz", "status": "resolved" }))
                .is_err()
        );
        wait_for(&received, 3);
        let run: serde_json::Value = serde_json::from_str(&received.lock().unwrap()[2]).unwrap();
        assert_eq!(run["type"], "run_agent_input");
        assert_eq!(run["input"]["threadId"], "s1");
        assert_eq!(run["input"]["messages"][0]["content"], "hello");
    }

    #[test]
    fn resume_covers_every_open_interrupt_before_it_goes_out() {
        let (port, received) = fake_daemon(
            r#"{"type":"browser_hello_ack","protocol_version":2}"#,
            vec![
                r#"{"type":"MESSAGES_SNAPSHOT","messages":[]}"#,
                r#"{"type":"RUN_FINISHED","threadId":"s1","runId":"r1","outcome":{"type":"interrupt","interrupts":[{"id":"a","reason":"tool_call","toolCallId":"a"},{"id":"b","reason":"tool_call","toolCallId":"b"}]}}"#,
            ],
        );
        let (sink, events, status, _) = sinks();
        let binding = Binding {
            port,
            token: "tok".into(),
        };
        let thread = Thread::open(
            &binding,
            "s1",
            "/proj",
            &ThreadOptions::default(),
            sink,
            status,
        )
        .unwrap();
        wait_for(&events, 1);
        thread
            .resume(serde_json::json!({ "interruptId": "a", "status": "resolved" }))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            received.lock().unwrap().len(),
            2,
            "one answer sends nothing yet"
        );
        thread
            .resume(serde_json::json!({ "interruptId": "b", "status": "cancelled", "payload": {"answers": []} }))
            .unwrap();
        wait_for(&received, 3);
        let frame: serde_json::Value = serde_json::from_str(&received.lock().unwrap()[2]).unwrap();
        assert_eq!(frame["input"]["resume"].as_array().unwrap().len(), 2);
        assert_eq!(
            frame["input"]["resume"][1]["payload"]["answers"],
            serde_json::json!([])
        );
        thread.interrupt().unwrap();
        wait_for(&received, 4);
        assert!(received.lock().unwrap()[3].contains("\"interrupt\""));
    }
}
