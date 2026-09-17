use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tauri::Manager;

mod agent;
mod browser_bridge;
mod cli_install;
mod config;
mod download;
mod env;
mod export;
mod gateway;
mod observability;
mod permissions;
mod process_manager;
mod projects;
mod scheduler;
mod screen_records;
mod skills;
mod stt;
mod tasks;
mod timeline;
mod tools;
mod tts_samples;
mod updates;

use observability::{StoredMetric, StoredSpan, start_collector};

pub(crate) struct AppState {
    processes: Arc<process_manager::ProcessManager>,
    scheduler_log: std::sync::Arc<std::sync::Mutex<VecDeque<String>>>,
    stored_traces: std::sync::Arc<std::sync::Mutex<VecDeque<StoredSpan>>>,
    stored_metrics: std::sync::Arc<std::sync::Mutex<VecDeque<StoredMetric>>>,
    screen_recording: std::sync::Mutex<Option<screen_records::RecordingHandle>>,
    browser_bridge: Arc<browser_bridge::Bridge>,
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(timeline::ProjectWatcher(std::sync::Mutex::new(None)))
        .manage(projects::GitWatcher(std::sync::Mutex::new(None)))
        .manage(AppState {
            processes: Arc::clone(&processes),
            scheduler_log: std::sync::Arc::new(std::sync::Mutex::new(VecDeque::new())),
            stored_traces,
            stored_metrics,
            screen_recording: std::sync::Mutex::new(None),
            browser_bridge: Arc::new(browser_bridge::Bridge::default()),
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            raise_overlay_above_dock(app);
            skills::install_bundled_skills();
            browser_bridge::start_if_enabled(&app.state::<AppState>(), app.handle().clone());
            {
                use tauri::Listener;
                let bridge = Arc::clone(&app.state::<AppState>().browser_bridge);
                app.listen_any("approval-resolved", move |event| {
                    if let Some(call_id) =
                        serde_json::from_str::<serde_json::Value>(event.payload())
                            .ok()
                            .and_then(|v| v.get("callId")?.as_str().map(String::from))
                    {
                        bridge.approval_resolved(&call_id);
                    }
                });
            }
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
            agent::send_user_message,
            agent::send_question_answers,
            agent::send_computer_use_control,
            agent::cancel_agent,
            agent::list_conversations,
            agent::get_conversation,
            agent::delete_conversation,
            agent::move_conversation,
            agent::read_projects,
            agent::write_projects,
            agent::list_models,
            config::get_auth,
            config::set_auth,
            config::get_config,
            config::set_config,
            config::set_default_model,
            export::export_desktop_file,
            export::export_desktop_github,
            export::import_desktop_file,
            export::import_desktop_github,
            export::read_desktop_data,
            export::save_desktop_snippets,
            export::save_skills_registry_url,
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
            tts_samples::list_voice_samples,
            tts_samples::add_voice_sample,
            tts_samples::save_voice_sample,
            tts_samples::delete_voice_sample,
            timeline::list_timelines,
            timeline::watch_project,
            timeline::read_timeline,
            timeline::write_timeline,
            timeline::reveal_project_file,
            timeline::add_project_video,
            timeline::import_project_file,
            timeline::list_project_media,
            timeline::prepare_content_tools,
            timeline::export_timeline,
            agent::list_a2a_agents,
            agent::add_a2a_agent,
            agent::remove_a2a_agent,
            agent::set_a2a_agent_model,
            agent::read_history,
            agent::append_history,
            agent::save_image,
            agent::save_audio,
            agent::save_upload,
            projects::create_project_dir,
            projects::move_project,
            projects::scan_git_repos,
            projects::clone_github_repo,
            projects::git_project_status,
            projects::sync_default_branch,
            projects::cleanup_project,
            projects::project_dir_exists,
            projects::open_in_vs_code,
            projects::refresh_project_context,
            projects::save_project_file,
            projects::list_project_files,
            skills::install_skill,
            skills::uninstall_skill,
            skills::list_installed_skills,
            tools::list_tools,
            tools::mcp_status,
            tools::a2a_status,
            tools::start_services,
            observability::get_traces,
            observability::get_metrics,
            permissions::computer_use_permission_status,
            permissions::set_computer_use_enabled,
            browser_bridge::browser_use_status,
            browser_bridge::set_browser_use_enabled,
            permissions::request_accessibility_permission,
            permissions::request_screen_recording_permission,
            screen_records::start_screen_recording,
            screen_records::stop_screen_recording,
            screen_records::screen_recording_status,
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
            tools::stop_services();
            if let Err(error) = state.processes.shutdown() {
                eprintln!("process shutdown during Tauri exit failed: {error}");
            }
            screen_records::stop_on_exit(&state);
            state.browser_bridge.stop();
        }
    });
}
