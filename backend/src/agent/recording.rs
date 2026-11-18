use crate::cli::events::AgentEvent;
use crate::cli::thread::Sink;
use crate::input_capture::{InputCapture, InputEvent, write_event};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The agent recording a session's process owns: input captured since
/// `RecordingStarted`, saved as `<recording>.events.jsonl` next to the MP4
/// when it stops, so every event lines up with the video.
pub(crate) struct AgentRecording {
    capture: InputCapture,
    inputs: Arc<Mutex<Vec<InputEvent>>>,
    path: Option<String>,
}

impl AgentRecording {
    pub(crate) fn start(path: String, sink: Sink) -> Self {
        let inputs = Arc::new(Mutex::new(Vec::new()));
        let capture = InputCapture::start({
            let inputs = Arc::clone(&inputs);
            move |input: InputEvent| {
                inputs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(input.clone());
                sink(AgentEvent::RecordingInput { input });
            }
        });
        AgentRecording {
            capture,
            inputs,
            path: Some(path),
        }
    }

    pub(crate) fn stop(self) {
        self.capture.stop();
        let Some(path) = self.path else { return };
        let log = Path::new(&path).with_extension("events.jsonl");
        let inputs = self.inputs.lock().unwrap_or_else(|p| p.into_inner());
        let write = std::fs::File::create(&log).and_then(|file| {
            let mut out = std::io::BufWriter::new(file);
            for input in inputs.iter() {
                write_event(&mut out, input)?;
            }
            out.flush()
        });
        if let Err(e) = write {
            eprintln!("agent recording: writing {}: {e}", log.display());
        }
    }
}
