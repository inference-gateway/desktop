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
mod stt;
mod updates;

use observability::{StoredMetric, StoredSpan, start_collector};

pub(crate) struct AppState {
    running_children: Mutex<std::collections::HashMap<String, std::process::Child>>,
    child_stdins: Mutex<std::collections::HashMap<String, std::process::ChildStdin>>,
    gateway_child: Mutex<Option<std::process::Child>>,
    stored_traces: std::sync::Arc<std::sync::Mutex<VecDeque<StoredSpan>>>,
    stored_metrics: std::sync::Arc<std::sync::Mutex<VecDeque<StoredMetric>>>,
}

pub fn run() {
    let stored_traces: Arc<Mutex<VecDeque<StoredSpan>>> = Arc::new(Mutex::new(VecDeque::new()));
    let stored_metrics: Arc<Mutex<VecDeque<StoredMetric>>> = Arc::new(Mutex::new(VecDeque::new()));
    let _collector = start_collector(Arc::clone(&stored_traces), Arc::clone(&stored_metrics));

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            running_children: Mutex::new(std::collections::HashMap::new()),
            child_stdins: Mutex::new(std::collections::HashMap::new()),
            gateway_child: Mutex::new(None),
            stored_traces,
            stored_metrics,
        })
        .invoke_handler(tauri::generate_handler![
            cli_install::check_and_install_cli,
            agent::send_message,
            agent::send_approval,
            agent::cancel_agent,
            agent::list_conversations,
            agent::get_conversation,
            agent::delete_conversation,
            agent::list_models,
            config::get_auth,
            config::set_auth,
            config::get_config,
            config::set_config,
            config::set_default_model,
            gateway::start_gateway,
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
            observability::get_traces,
            observability::get_metrics,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let state = app_handle.state::<AppState>();
                if let Ok(mut guard) = state.gateway_child.lock()
                    && let Some(mut child) = guard.take()
                {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                if let Ok(mut children) = state.running_children.lock() {
                    for (_, mut child) in children.drain() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
        });
}
