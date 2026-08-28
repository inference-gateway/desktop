use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tauri::Manager;

mod agent;
mod cli_install;
mod config;
mod download;
mod env;
mod gateway;
mod observability;
mod permissions;
mod process_manager;
mod projects;
mod scheduler;
mod skills;
mod stt;
mod tasks;
mod updates;

use observability::{StoredMetric, StoredSpan, start_collector};

pub(crate) struct AppState {
    processes: Arc<process_manager::ProcessManager>,
    scheduler_log: std::sync::Arc<std::sync::Mutex<VecDeque<String>>>,
    stored_traces: std::sync::Arc<std::sync::Mutex<VecDeque<StoredSpan>>>,
    stored_metrics: std::sync::Arc<std::sync::Mutex<VecDeque<StoredMetric>>>,
}

// Always-on-top (NSFloatingWindowLevel) still draws under the Dock; Tauri has no
// window-level API, so set NSStatusWindowLevel (25) directly.
#[cfg(target_os = "macos")]
fn raise_overlay_above_dock(app: &tauri::App) {
    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(ns_window) = overlay.ns_window() else {
        return;
    };
    unsafe {
        let win = ns_window as *mut objc2::runtime::AnyObject;
        let _: () = objc2::msg_send![win, setLevel: 25isize];
        let _: () = objc2::msg_send![win, setAccessibilityElement: false];
    }
}

pub fn run() {
    let stored_traces: Arc<Mutex<VecDeque<StoredSpan>>> = Arc::new(Mutex::new(VecDeque::new()));
    let stored_metrics: Arc<Mutex<VecDeque<StoredMetric>>> = Arc::new(Mutex::new(VecDeque::new()));
    let _collector = start_collector(Arc::clone(&stored_traces), Arc::clone(&stored_metrics));
    let processes = Arc::new(process_manager::ProcessManager::new());

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            processes: Arc::clone(&processes),
            scheduler_log: std::sync::Arc::new(std::sync::Mutex::new(VecDeque::new())),
            stored_traces,
            stored_metrics,
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            raise_overlay_above_dock(app);
            if config::read_config().schedule_enabled {
                let state = app.state::<AppState>();
                if let Err(e) = scheduler::spawn_daemon(&state) {
                    eprintln!("scheduler autostart failed: {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cli_install::check_and_install_cli,
            agent::send_message,
            agent::send_approval,
            agent::send_computer_use_control,
            agent::cancel_agent,
            agent::list_conversations,
            agent::get_conversation,
            agent::delete_conversation,
            agent::read_projects,
            agent::write_projects,
            agent::list_models,
            config::get_auth,
            config::set_auth,
            config::get_config,
            config::set_config,
            config::set_default_model,
            gateway::start_gateway,
            scheduler::start_scheduler,
            scheduler::stop_scheduler,
            scheduler::get_scheduler_status,
            scheduler::get_scheduler_log,
            scheduler::list_schedules,
            scheduler::github_auth_status,
            scheduler::github_owners,
            scheduler::github_repo_exists,
            scheduler::github_create_repo,
            scheduler::github_list_secrets,
            scheduler::github_set_secret,
            scheduler::github_list_repos,
            tasks::github_check_workflow,
            tasks::github_install_workflow,
            tasks::github_bump_workflow,
            tasks::github_list_task_issues,
            tasks::github_list_task_pulls,
            tasks::github_list_workflow_runs,
            tasks::github_create_task_issue,
            tasks::github_run_task_issue,
            scheduler::open_url,
            updates::check_updates,
            updates::install_desktop_update,
            stt::stt_status,
            stt::prepare_stt,
            stt::transcribe_audio,
            agent::list_a2a_agents,
            agent::add_a2a_agent,
            agent::remove_a2a_agent,
            agent::set_a2a_agent_model,
            agent::read_history,
            agent::append_history,
            agent::save_image,
            agent::save_upload,
            projects::create_project_dir,
            projects::scan_git_repos,
            projects::clone_github_repo,
            projects::git_project_names,
            projects::project_dir_exists,
            projects::refresh_project_context,
            projects::save_project_file,
            projects::list_project_files,
            skills::install_skill,
            skills::uninstall_skill,
            skills::list_installed_skills,
            observability::get_traces,
            observability::get_metrics,
            permissions::computer_use_permission_status,
            permissions::set_computer_use_enabled,
            permissions::request_accessibility_permission,
            permissions::request_screen_recording_permission,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    let signal_processes = Arc::clone(&processes);
    let signal_app = app.handle().clone();
    ctrlc::set_handler(move || {
        if let Err(error) = signal_processes.shutdown() {
            eprintln!("process shutdown after signal failed: {error}");
        }
        signal_app.exit(0);
    })
    .expect("failed to install SIGINT/SIGTERM handler");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let state = app_handle.state::<AppState>();
            if let Err(error) = state.processes.shutdown() {
                eprintln!("process shutdown during Tauri exit failed: {error}");
            }
        }
    });
}
