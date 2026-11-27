use crate::cli::daemon::{Binding, lock};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, mpsc};
use std::time::Duration;
use tauri::Manager;
use tauri::ipc::Channel;

/// Listing and following a room answer from the daemon's own state.
const ROOM_TIMEOUT: Duration = Duration::from_secs(15);

/// A post that pings nobody waits on the router's decision model first.
const POST_TIMEOUT: Duration = Duration::from_secs(90);

/// The rooms the window follows, each by the sender that keeps its socket open.
/// Dropping a sender ends that follow and closes its connection.
#[derive(Default)]
pub(crate) struct RoomFollows(Mutex<HashMap<String, mpsc::Sender<String>>>);

fn binding(app: &tauri::AppHandle) -> Result<Binding, String> {
    let state = app.state::<crate::AppState>();
    crate::cli::daemon::ensure_running(&state.processes, &state.scheduler_log)
}

/// The frame of type `kind`, `None` for any other.
fn answer_of(frame: &Value, kind: &str) -> Option<Value> {
    (frame["type"] == kind).then(|| frame.clone())
}

/// The rooms the `rooms` answer lists, each with its project and members.
fn rooms_of(frame: &Value) -> Option<Vec<Value>> {
    answer_of(frame, "rooms").map(|f| f["rooms"].as_array().cloned().unwrap_or_default())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// Every room of `rooms.yaml`, the default `public` room included, with each
/// member's session, role and activity.
#[tauri::command]
pub(crate) async fn list_rooms(app: tauri::AppHandle) -> Result<Vec<Value>, String> {
    blocking(move || {
        crate::cli::daemon::ask(
            || binding(&app),
            json!({ "type": "list_rooms" }),
            ROOM_TIMEOUT,
            rooms_of,
        )
    })
    .await
}

/// Posts `text` to `room` as the user and answers the daemon's `room_posted`:
/// the routed post and the notices for its author.
#[tauri::command]
pub(crate) async fn post_room(
    app: tauri::AppHandle,
    room: String,
    text: String,
) -> Result<Value, String> {
    blocking(move || {
        crate::cli::daemon::ask(
            || binding(&app),
            json!({ "type": "post_room", "room": room, "text": text }),
            POST_TIMEOUT,
            |frame| answer_of(frame, "room_posted"),
        )
    })
    .await
}

/// Follows `room` on a connection of its own and answers its `room_snapshot`.
/// Every later frame of the room goes to `on_event` until `leave_room`, and a
/// second follow of the same room replaces the first.
#[tauri::command]
pub(crate) async fn follow_room(
    app: tauri::AppHandle,
    room: String,
    on_event: Channel<Value>,
) -> Result<Value, String> {
    let handle = app.clone();
    let name = room.clone();
    let (snapshot, sender) = blocking(move || {
        crate::cli::thread::subscribe(
            &binding(&handle)?,
            json!({ "type": "follow_room", "room": name }),
            "room_snapshot",
            ROOM_TIMEOUT,
            move |text| {
                if let Ok(frame) = serde_json::from_str::<Value>(text) {
                    let _ = on_event.send(frame);
                }
            },
        )
    })
    .await?;
    lock(&app.state::<RoomFollows>().0).insert(room, sender);
    Ok(snapshot)
}

/// Stops following `room`: the daemon gets `leave_room` and the connection closes.
#[tauri::command]
pub(crate) fn leave_room(follows: tauri::State<'_, RoomFollows>, room: String) {
    if let Some(sender) = lock(&follows.0).remove(&room) {
        let _ = sender.send(json!({ "type": "leave_room", "room": room }).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rooms_answer_lists_its_rooms() {
        let listed = json!({ "type": "rooms", "rooms": [{ "name": "public", "members": [] }] });
        assert_eq!(
            rooms_of(&listed),
            Some(vec![json!({ "name": "public", "members": [] })])
        );
        assert_eq!(rooms_of(&json!({ "type": "rooms" })), Some(vec![]));
        assert_eq!(rooms_of(&json!({ "type": "room_posted" })), None);
    }

    #[test]
    fn only_the_awaited_frame_answers() {
        let posted = json!({ "type": "room_posted", "room": "public", "post": { "id": "1" } });
        assert_eq!(answer_of(&posted, "room_posted"), Some(posted.clone()));
        assert_eq!(answer_of(&posted, "room_snapshot"), None);
    }
}
