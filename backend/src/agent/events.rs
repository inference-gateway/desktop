use crate::input_capture::{InputCapture, InputEvent, write_event};
use crate::observability::json_val_i64;
use crate::thread::Sink;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub(crate) enum AgentEvent {
    #[allow(dead_code)]
    SessionId {
        session_id: String,
    },
    AssistantMessage {
        content: String,
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCallInfo>,
        message_id: Option<String>,
    },
    ToolResult {
        content: String,
        tool_call_id: String,
    },
    /// A user message another client of the thread sent, echoed by the daemon.
    UserMessage {
        content: String,
    },
    ApprovalRequest {
        tool_name: String,
        tool_args: String,
        tool_call_id: String,
    },
    UserQuestionRequest {
        tool_call_id: String,
        questions: serde_json::Value,
    },
    /// The thread started a run, whichever client or worker started it.
    RunStarted,
    AgentError {
        message: String,
    },
    RawLine {
        line: String,
    },
    Done {
        exit_code: i32,
        stderr: String,
    },
    TokenUsage {
        input: i64,
        output: i64,
        cached_read: i64,
        total_tool_calls: i64,
        last_input: i64,
        context_window: i64,
        cost: f64,
    },
    Cancelled,
    /// A tool call the CLI cancelled before it ran.
    ToolCancelled {
        tool_call_id: String,
    },
    /// The CLI optimized the conversation to fit the context window, from
    /// `original_count` messages down to `optimized_count`.
    ConversationOptimized {
        message: String,
        original_count: u64,
        optimized_count: u64,
    },
    AgentStatus {
        name: String,
        state: String,
        message: String,
        done: u64,
        total: u64,
    },
    BackgroundNote {
        content: String,
    },
    BackgroundTasks {
        running: u64,
        jobs: Vec<BackgroundJob>,
    },
    /// The one-line next step the CLI suggests after a turn - empty means none.
    NextStepSuggestion {
        text: String,
    },
    RecordingStarted {
        path: String,
        area: RecordedArea,
    },
    ComputerUseAction {
        tool_call_id: String,
        action: String,
        x: Option<f64>,
        y: Option<f64>,
        screen_width: f64,
        screen_height: f64,
    },
    RecordingInput {
        input: InputEvent,
    },
    RecordingStopped,
}

/// What an agent recording captures, in the CLI's Computer screenshot frame
/// space: the rectangle plus the frame size of the whole primary screen, so a
/// window can scale it to its own pixels.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct RecordedArea {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    frame_width: i64,
    frame_height: i64,
}

/// Reads the `screenRecording` state key: the MP4 path and the recorded area
/// while a recording runs (`region` in frame space with `frameWidth` and
/// `frameHeight`), None once it is off.
fn recording_area(state: &serde_json::Value) -> Option<(String, RecordedArea)> {
    if state.get("active")?.as_bool() != Some(true) {
        return None;
    }
    let region = state.get("region")?;
    let int = |v: &serde_json::Value, k: &str| v.get(k).and_then(|n| n.as_i64());
    Some((
        state.get("path")?.as_str()?.to_string(),
        RecordedArea {
            x: int(region, "x")?,
            y: int(region, "y")?,
            width: int(region, "width")?,
            height: int(region, "height")?,
            frame_width: int(state, "frameWidth")?,
            frame_height: int(state, "frameHeight")?,
        },
    ))
}

/// The agent recording a session's process owns: input captured since
/// `RecordingStarted`, saved as `<recording>.events.jsonl` next to the MP4
/// when it stops, so every event lines up with the video.
pub(super) struct AgentRecording {
    capture: InputCapture,
    inputs: Arc<Mutex<Vec<InputEvent>>>,
    path: Option<String>,
}

impl AgentRecording {
    pub(super) fn start(path: String, sink: Sink) -> Self {
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

    pub(super) fn stop(self) {
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

/// One supervised CLI job, from the `background_tasks` AG-UI snapshot or a
/// `tasks` answer. Only the answer carries `output`, what the job printed so far.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct BackgroundJob {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) detail: String,
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) output: String,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct ToolCallInfo {
    id: String,
    pub(crate) name: String,
    args: String,
}

/// Folds AG-UI event-stream lines into complete AgentEvents, accumulating
/// message/tool-call triads internally.
pub(crate) struct AgentParser {
    session_id: Option<String>,
    msg_from_user: bool,
    message_id: Option<String>,
    tc_id: String,
    tc_name: String,
    tc_args: String,
    /// Name and args of every tool call the run streamed, by id, for the
    /// interrupt that names only the toolCallId it waits on.
    tool_calls: HashMap<String, (String, String)>,
    /// The cumulative input tokens the last usage carried, so the next one
    /// yields the last request's input for the context meter.
    last_total_input: i64,
    /// The `screenRecording` state is active. `RUN_FINISHED` then reports it
    /// stopped: the CLI finalizes it on exit with no `active: false` patch after
    /// the terminal event.
    recording: bool,
}

/// Reads the text delta of a streaming message event, tolerating producers that
/// use `content` instead of `delta`.
pub(crate) fn delta_of(val: &serde_json::Value) -> Option<&str> {
    val.get("delta")
        .or_else(|| val.get("content"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

fn message_id_of(val: &serde_json::Value) -> Option<String> {
    val.get("messageId")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// The `outcome.type` of a run terminal event: `success`, `interrupt` or a
/// stopped turn's `cancelled`. A cancelled terminal carries no stats, so it
/// must not synthesize a zero usage event.
pub(crate) fn run_outcome(val: &serde_json::Value) -> Option<&str> {
    val.get("outcome")
        .and_then(|o| o.get("type"))
        .and_then(|t| t.as_str())
}

/// Sums the AG-UI `usage` list (one TokenUsage per model, cumulative across
/// the session) and reads what it has no field for from the run's `result`:
/// tool calls, cost and the context window.
fn usage_event(
    usage: Option<&serde_json::Value>,
    result: Option<&serde_json::Value>,
    last_total_input: &mut i64,
) -> AgentEvent {
    let sum = |key: &str| {
        usage
            .and_then(|u| u.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|u| u.get(key))
                    .map(json_val_i64)
                    .sum()
            })
            .unwrap_or(0)
    };
    let from_result = |key: &str| {
        result
            .and_then(|r| r.get(key))
            .map(json_val_i64)
            .unwrap_or(0)
    };
    let input = sum("inputTokens").max(from_result("inputTokens"));
    let last_input = match from_result("lastInputTokens") {
        0 if input > *last_total_input => input - *last_total_input,
        n => n,
    };
    *last_total_input = input;
    AgentEvent::TokenUsage {
        input,
        output: sum("outputTokens").max(from_result("outputTokens")),
        cached_read: sum("cachedInputTokens").max(from_result("cacheReadTokens")),
        total_tool_calls: from_result("totalToolCalls"),
        last_input,
        context_window: from_result("contextWindow"),
        cost: result
            .and_then(|r| r.get("cost"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    }
}

impl AgentParser {
    pub(crate) fn new(session_id: Option<String>) -> Self {
        Self {
            session_id,
            msg_from_user: false,
            message_id: None,
            tool_calls: HashMap::new(),
            last_total_input: 0,
            tc_id: String::new(),
            tc_name: String::new(),
            tc_args: String::new(),
            recording: false,
        }
    }

    /// Emits an assistant text delta, or the user message the daemon echoes
    /// when the current message carries the user role.
    fn assistant_text(&self, val: &serde_json::Value) -> Option<AgentEvent> {
        if self.msg_from_user {
            return Some(AgentEvent::UserMessage {
                content: delta_of(val)?.to_string(),
            });
        }
        Some(AgentEvent::AssistantMessage {
            content: delta_of(val)?.to_string(),
            reasoning_content: None,
            tool_calls: Vec::new(),
            message_id: message_id_of(val).or_else(|| self.message_id.clone()),
        })
    }

    /// Take the accumulated session_id out.
    #[cfg(test)]
    fn take_session_id(&mut self) -> Option<String> {
        self.session_id.take()
    }

    pub(crate) fn parse_line(&mut self, line: &str) -> Option<AgentEvent> {
        let val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                return Some(AgentEvent::RawLine {
                    line: line.to_string(),
                });
            }
        };

        let event_type = match val.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                return Some(AgentEvent::RawLine {
                    line: line.to_string(),
                });
            }
        };

        match event_type {
            "RUN_STARTED" => {
                let sid = val.get("threadId").and_then(|v| v.as_str()).unwrap_or("");
                if !sid.is_empty() && self.session_id.is_none() {
                    self.session_id = Some(sid.to_string());
                }
                Some(AgentEvent::RunStarted)
            }
            "MESSAGES_SNAPSHOT" => None,
            "STATE_SNAPSHOT" => self.state(val.get("snapshot")?),
            "STATE_DELTA" => {
                let mut state = serde_json::Map::new();
                for op in val.get("delta")?.as_array()? {
                    if let (Some(path), Some(value)) =
                        (op.get("path").and_then(|p| p.as_str()), op.get("value"))
                    {
                        state.insert(path.trim_start_matches('/').to_string(), value.clone());
                    }
                }
                self.state(&serde_json::Value::Object(state))
            }
            "ACTIVITY_SNAPSHOT" => self.activity(&val),
            "TEXT_MESSAGE_START" => {
                self.message_id = message_id_of(&val);
                self.msg_from_user = val.get("role").and_then(|v| v.as_str()) == Some("user");
                self.assistant_text(&val)
            }
            "TEXT_MESSAGE_CONTENT" => self.assistant_text(&val),
            "TEXT_MESSAGE_END" => {
                self.msg_from_user = false;
                self.message_id = None;
                None
            }
            // Reasoning/thinking phases are assistant-only; each content event is
            // one streamed delta. THINKING_* are the deprecated aliases of the
            // REASONING_* events - accept both.
            "REASONING_MESSAGE_CONTENT" | "THINKING_TEXT_MESSAGE_CONTENT" => {
                delta_of(&val).map(|d| AgentEvent::AssistantMessage {
                    content: String::new(),
                    reasoning_content: Some(d.to_string()),
                    tool_calls: Vec::new(),
                    message_id: message_id_of(&val).or_else(|| self.message_id.clone()),
                })
            }
            "REASONING_MESSAGE_START"
            | "REASONING_START"
            | "THINKING_TEXT_MESSAGE_START"
            | "THINKING_START" => {
                self.message_id = message_id_of(&val);
                None
            }
            "REASONING_MESSAGE_END"
            | "REASONING_END"
            | "THINKING_TEXT_MESSAGE_END"
            | "THINKING_END" => {
                self.message_id = None;
                None
            }
            "TOOL_CALL_START" => {
                self.tc_id = val
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.tc_name = val
                    .get("toolCallName")
                    .or_else(|| val.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.tc_args = String::new();
                None
            }
            "TOOL_CALL_ARGS" => {
                if let Some(args) = val
                    .get("delta")
                    .or_else(|| val.get("args"))
                    .and_then(|v| v.as_str())
                {
                    self.tc_args.push_str(args);
                }
                None
            }
            "TOOL_CALL_END" => {
                if self.tc_id.is_empty() {
                    return None;
                }
                self.tool_calls.insert(
                    self.tc_id.clone(),
                    (self.tc_name.clone(), self.tc_args.clone()),
                );
                Some(AgentEvent::AssistantMessage {
                    content: String::new(),
                    reasoning_content: None,
                    tool_calls: vec![ToolCallInfo {
                        id: std::mem::take(&mut self.tc_id),
                        name: std::mem::take(&mut self.tc_name),
                        args: std::mem::take(&mut self.tc_args),
                    }],
                    message_id: None,
                })
            }
            "TOOL_CALL_RESULT" => {
                let content = val
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = val
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if content.is_empty() {
                    return None;
                }
                Some(AgentEvent::ToolResult {
                    content,
                    tool_call_id,
                })
            }
            "CUSTOM" => {
                match val.get("name").and_then(|v| v.as_str()) {
                    Some("agent_status") => {
                        let v = val.get("value");
                        let field = |k: &str| {
                            v.and_then(|v| v.get(k))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        };
                        let num = |k: &str| {
                            v.and_then(|v| v.get(k))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                        };
                        return Some(AgentEvent::AgentStatus {
                            name: field("name"),
                            state: field("state"),
                            message: field("message"),
                            done: num("done"),
                            total: num("total"),
                        });
                    }
                    Some("queued_message") => {
                        let content = val
                            .get("value")
                            .and_then(|v| v.get("content"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        return Some(AgentEvent::BackgroundNote { content });
                    }
                    _ => {}
                }
                Some(AgentEvent::RawLine {
                    line: line.to_string(),
                })
            }
            "RUN_FINISHED" => {
                self.flush_message();
                if run_outcome(&val) == Some("interrupt")
                    && let Some(interrupt) = val
                        .get("outcome")
                        .and_then(|o| o.get("interrupts")?.as_array()?.first())
                {
                    return self.interrupt(interrupt);
                }
                if std::mem::take(&mut self.recording) {
                    return Some(AgentEvent::RecordingStopped);
                }
                if run_outcome(&val) == Some("cancelled") {
                    return None;
                }
                Some(usage_event(
                    val.get("usage"),
                    val.get("result").or_else(|| val.get("stats")),
                    &mut self.last_total_input,
                ))
            }
            "RUN_ERROR" => {
                self.flush_message();
                let message = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(AgentEvent::AgentError { message })
            }
            _ => Some(AgentEvent::RawLine {
                line: line.to_string(),
            }),
        }
    }

    fn flush_message(&mut self) {
        self.msg_from_user = false;
        self.message_id = None;
    }

    /// Follows the run's state keys: `screenRecording` starting or stopping a
    /// recording, `usage` streaming the token totals, `backgroundTasks`, and
    /// `nextStepSuggestion`. A snapshot reports the first of them it carries
    /// something for, so the suggestion only lands through its own delta.
    fn state(&mut self, state: &serde_json::Value) -> Option<AgentEvent> {
        if let Some(recording) = state.get("screenRecording") {
            if let Some((path, area)) = recording_area(recording) {
                self.recording = true;
                return Some(AgentEvent::RecordingStarted { path, area });
            }
            if std::mem::take(&mut self.recording) {
                return Some(AgentEvent::RecordingStopped);
            }
        }
        if let Some(usage) = state
            .get("usage")
            .filter(|u| u.as_array().is_some_and(|l| !l.is_empty()))
        {
            return Some(usage_event(Some(usage), None, &mut self.last_total_input));
        }
        if let Some(tasks) = state.get("backgroundTasks") {
            let running = tasks.get("running").and_then(|v| v.as_u64()).unwrap_or(0);
            let jobs = tasks
                .get("jobs")
                .cloned()
                .and_then(|j| serde_json::from_value(j).ok())
                .unwrap_or_default();
            return Some(AgentEvent::BackgroundTasks { running, jobs });
        }
        let text = state.get("nextStepSuggestion")?.as_str()?;
        Some(AgentEvent::NextStepSuggestion {
            text: text.to_owned(),
        })
    }

    /// Maps the interrupt a run suspended on: an approval for the tool call
    /// it names, or the AskUserQuestion form whose questions are the args of
    /// the call with the interrupt's id.
    fn interrupt(&self, interrupt: &serde_json::Value) -> Option<AgentEvent> {
        let id = interrupt.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let tool_call_id = interrupt
            .get("toolCallId")
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();
        let (name, args) = self
            .tool_calls
            .get(&tool_call_id)
            .cloned()
            .unwrap_or_default();
        match interrupt.get("reason").and_then(|v| v.as_str())? {
            "tool_call" => Some(AgentEvent::ApprovalRequest {
                tool_name: name,
                tool_args: args,
                tool_call_id,
            }),
            "input_required" => {
                let questions = serde_json::from_str::<serde_json::Value>(&args)
                    .ok()
                    .and_then(|a| a.get("questions").cloned())
                    .unwrap_or_else(|| serde_json::json!([]));
                Some(AgentEvent::UserQuestionRequest {
                    tool_call_id,
                    questions,
                })
            }
            _ => None,
        }
    }

    /// Maps an ACTIVITY_SNAPSHOT: a computer-use action in screen coordinates,
    /// a local agent's startup progress, a tool call cancelled before it ran,
    /// or a finished conversation optimization. Every other tool progress
    /// status is ignored.
    fn activity(&self, val: &serde_json::Value) -> Option<AgentEvent> {
        let content = val.get("content")?;
        let text = |k: &str| {
            content
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let count = |k: &str| content.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        match val.get("activityType").and_then(|v| v.as_str())? {
            "computer_use" => {
                let num = |k: &str| content.get(k).and_then(|v| v.as_f64());
                Some(AgentEvent::ComputerUseAction {
                    tool_call_id: text("toolCallId"),
                    action: text("action"),
                    x: num("x"),
                    y: num("y"),
                    screen_width: num("screenWidth").unwrap_or(0.0),
                    screen_height: num("screenHeight").unwrap_or(0.0),
                })
            }
            "agent_status" => Some(AgentEvent::AgentStatus {
                name: text("name"),
                state: text("state"),
                message: text("message"),
                done: count("done"),
                total: count("total"),
            }),
            "tool_progress" if text("status") == "cancelled" => Some(AgentEvent::ToolCancelled {
                tool_call_id: text("toolCallId"),
            }),
            "optimization_status" => Some(AgentEvent::ConversationOptimized {
                message: text("message"),
                original_count: count("originalCount"),
                optimized_count: count("optimizedCount"),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(lines: &[&str]) -> (Vec<AgentEvent>, Option<String>) {
        let mut p = AgentParser::new(None);
        let events: Vec<AgentEvent> = lines
            .iter()
            .filter_map(|l| p.parse_line(l.trim()))
            .collect();
        let sid = p.take_session_id();
        (events, sid)
    }

    #[test]
    fn test_parse_text_message_streams_each_content_delta() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","content":"Hello.","contentType":"text"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","content":" I am an AI.","contentType":"text"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(
            events.len(),
            2,
            "each content event streams as its own delta"
        );
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { content, message_id, .. }
            if content == "Hello." && message_id.as_deref() == Some("msg-1"))
        );
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, message_id, .. }
            if content == " I am an AI." && message_id.as_deref() == Some("msg-1"))
        );
    }

    #[test]
    fn test_parse_reasoning_streams_as_reasoning_content() {
        let lines = &[
            r#"{"type":"REASONING_MESSAGE_START","messageId":"r1","role":"assistant"}"#,
            r#"{"type":"REASONING_MESSAGE_CONTENT","messageId":"r1","delta":"Let me think. "}"#,
            r#"{"type":"REASONING_MESSAGE_CONTENT","messageId":"r1","delta":"Step 1."}"#,
            r#"{"type":"REASONING_MESSAGE_END","messageId":"r1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a1","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a1","delta":"Answer."}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 3, "two reasoning deltas + one content delta");
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { reasoning_content: Some(r), content, .. } if r == "Let me think. " && content.is_empty())
        );
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { reasoning_content: Some(r), .. } if r == "Step 1.")
        );
        assert!(
            matches!(&events[2], AgentEvent::AssistantMessage { content, reasoning_content: None, .. } if content == "Answer.")
        );
    }

    #[test]
    fn test_parse_thinking_alias_maps_to_reasoning() {
        let lines =
            &[r#"{"type":"THINKING_TEXT_MESSAGE_CONTENT","messageId":"t1","delta":"pondering"}"#];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { reasoning_content: Some(r), .. } if r == "pondering")
        );
    }

    #[test]
    fn test_parse_text_message_no_content_returns_none() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_agui_real_turn_reports_user_echo_and_reads_delta() {
        let lines = &[
            r#"{"type":"RUN_STARTED","threadId":"sess-1","runId":"run-1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"u1","role":"user"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"u1","delta":"what is up?"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"u1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a1","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a1","delta":"All good!"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a1"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"sess-1","runId":"run-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(
            events.len(),
            4,
            "expected RunStarted + UserMessage + AssistantMessage + TokenUsage"
        );
        assert!(matches!(&events[0], AgentEvent::RunStarted));
        assert!(
            matches!(&events[1], AgentEvent::UserMessage { content } if content == "what is up?"),
            "the user echo is what another client of the thread sent"
        );
        assert!(
            matches!(&events[2], AgentEvent::AssistantMessage { content, .. } if content == "All good!"),
            "assistant reply must render from `delta`"
        );
        assert!(
            matches!(&events[3], AgentEvent::TokenUsage { .. }),
            "RUN_FINISHED emits TokenUsage"
        );
    }

    #[test]
    fn test_parse_agui_resumed_turn_renders_reply_after_snapshot() {
        let lines = &[
            r#"{"type":"RUN_STARTED","threadId":"sess-1","runId":"run-2"}"#,
            r#"{"type":"MESSAGES_SNAPSHOT","messages":[{"role":"user","content":"earlier"},{"role":"assistant","content":"earlier reply"}]}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"u2","role":"user"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"u2","delta":"and now?"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"u2"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a2","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a2","delta":"Here is more."}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a2"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"sess-1","runId":"run-2"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(
            events.len(),
            4,
            "expected RunStarted + UserMessage + AssistantMessage + TokenUsage"
        );
        assert!(matches!(&events[0], AgentEvent::RunStarted));
        assert!(matches!(&events[1], AgentEvent::UserMessage { content } if content == "and now?"));
        assert!(
            matches!(&events[2], AgentEvent::AssistantMessage { content, .. } if content == "Here is more."),
            "resumed reply must render from `delta`"
        );
        assert!(
            matches!(&events[3], AgentEvent::TokenUsage { .. }),
            "RUN_FINISHED emits TokenUsage"
        );
    }

    #[test]
    fn test_parse_agui_cancelled_run_finished_reports_no_usage() {
        let lines = &[
            r#"{"type":"RUN_STARTED","threadId":"sess-1","runId":"run-1"}"#,
            r#"{"type":"TEXT_MESSAGE_START","messageId":"a1","role":"assistant"}"#,
            r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"a1","delta":"partial"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"a1"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"sess-1","runId":"run-1","outcome":{"type":"cancelled"}}"#,
        ];
        let (events, _) = parse_all(lines);
        assert!(matches!(&events[0], AgentEvent::RunStarted));
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, .. } if content == "partial")
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::TokenUsage { .. })),
            "a stopped turn's terminal event carries no usage: synthesizing zeros would reset the session meters"
        );
    }

    #[test]
    fn test_parse_tool_call_triad() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1"}"#,
            r#"{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-1","toolCallName":"Bash"}"#,
            r#"{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-1","delta":"{\"command\":\"ls\"}"}"#,
            r#"{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-1"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-1");
                assert_eq!(tool_calls[0].name, "Bash");
                assert_eq!(tool_calls[0].args, "{\"command\":\"ls\"}");
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn test_parse_tool_call_with_text() {
        let lines = &[
            r#"{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1","content":"Let me check that file."}"#,
            r#"{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-1","name":"read_file"}"#,
            r#"{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-1","args":"{\"path\":\"/tmp/test\"}"}"#,
            r#"{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-1"}"#,
            r#"{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-2","name":"list_dir"}"#,
            r#"{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-2","args":"{\"path\":\"/tmp\"}"}"#,
            r#"{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-2"}"#,
            r#"{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}"#,
        ];
        let (events, _) = parse_all(lines);
        assert_eq!(events.len(), 3);
        // Streamed text arrives first (on TEXT_MESSAGE_START), then each tool call.
        assert!(
            matches!(&events[0], AgentEvent::AssistantMessage { content, tool_calls, .. }
            if content == "Let me check that file." && tool_calls.is_empty())
        );
        match &events[1] {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-1");
                assert_eq!(tool_calls[0].name, "read_file");
            }
            _ => panic!("expected AssistantMessage for first tool call"),
        }
        match &events[2] {
            AgentEvent::AssistantMessage {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call-2");
                assert_eq!(tool_calls[0].name, "list_dir");
            }
            _ => panic!("expected AssistantMessage for second tool call"),
        }
    }

    #[test]
    fn test_parse_tool_call_result() {
        let (events, _) = parse_all(&[
            r#"{"type":"TOOL_CALL_RESULT","toolCallId":"call-1","content":"{\"tool_name\":\"read_file\",\"success\":true}"}"#,
        ]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolResult { content, tool_call_id }
            if content == "{\"tool_name\":\"read_file\",\"success\":true}" && tool_call_id == "call-1")
        );
    }

    #[test]
    fn test_parse_tool_call_result_empty_returns_none() {
        let (events, _) =
            parse_all(&[r#"{"type":"TOOL_CALL_RESULT","toolCallId":"call-1","content":""}"#]);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_run_error() {
        let (events, _) = parse_all(&[r#"{"type":"RUN_ERROR","message":"Gateway unreachable"}"#]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::AgentError { message } if message == "Gateway unreachable")
        );
    }

    #[test]
    fn test_parse_run_finished_returns_token_usage() {
        let (events, _) = parse_all(&[r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r"}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AgentEvent::TokenUsage {
                input: 0,
                output: 0,
                last_input: 0,
                context_window: 0,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_run_finished_reads_usage_and_result() {
        let (events, _) = parse_all(&[
            r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","usage":[{"model":"a","inputTokens":4000,"outputTokens":300,"cachedInputTokens":3000},{"model":"b","inputTokens":821,"outputTokens":10,"cachedInputTokens":100}],"result":{"totalToolCalls":3,"cost":0.042,"lastInputTokens":912,"contextWindow":128000}}"#,
        ]);
        let AgentEvent::TokenUsage {
            input,
            output,
            cached_read,
            total_tool_calls,
            last_input,
            context_window,
            cost,
        } = &events[0]
        else {
            panic!("expected TokenUsage");
        };
        assert_eq!(
            (
                *input,
                *output,
                *cached_read,
                *total_tool_calls,
                *last_input,
                *context_window
            ),
            (4821, 310, 3100, 3, 912, 128000)
        );
        assert!((cost - 0.042).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_state_snapshot_without_usage_reports_only_the_tasks() {
        let (events, _) = parse_all(&[
            r#"{"type":"STATE_SNAPSHOT","snapshot":{"todos":[{"id":"1","content":"test"}],"usage":[],"backgroundTasks":{"running":0,"jobs":[]},"nextStepSuggestion":"","screenRecording":{"active":false}}}"#,
        ]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::BackgroundTasks { running: 0, jobs } if jobs.is_empty())
        );
    }

    #[test]
    fn test_parse_state_next_step_suggestion_delta() {
        let (events, _) = parse_all(&[
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/nextStepSuggestion","value":"run the failing test"}]}"#,
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/nextStepSuggestion","value":""}]}"#,
        ]);
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], AgentEvent::NextStepSuggestion { text } if text == "run the failing test")
        );
        assert!(matches!(&events[1], AgentEvent::NextStepSuggestion { text } if text.is_empty()));
    }

    #[test]
    fn test_parse_state_usage_streams_the_last_request_input() {
        let (events, _) = parse_all(&[
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/usage","value":[{"model":"m","inputTokens":1200,"outputTokens":80,"cachedInputTokens":300}]}]}"#,
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/usage","value":[{"model":"m","inputTokens":1840,"outputTokens":120,"cachedInputTokens":300}]}]}"#,
        ]);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            AgentEvent::TokenUsage {
                input: 1200,
                output: 80,
                cached_read: 300,
                last_input: 1200,
                ..
            }
        ));
        assert!(matches!(
            &events[1],
            AgentEvent::TokenUsage {
                input: 1840,
                output: 120,
                last_input: 640,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_interrupts_name_the_tool_call_and_the_questions() {
        let (events, _) = parse_all(&[
            r#"{"type":"TOOL_CALL_START","toolCallId":"call-1","toolCallName":"read_file"}"#,
            r#"{"type":"TOOL_CALL_ARGS","toolCallId":"call-1","delta":"{\"path\":\"/tmp/test\"}"}"#,
            r#"{"type":"TOOL_CALL_END","toolCallId":"call-1"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r1","outcome":{"type":"interrupt","interrupts":[{"id":"call-1","reason":"tool_call","toolCallId":"call-1"}]}}"#,
            r#"{"type":"TOOL_CALL_START","toolCallId":"call-2","toolCallName":"AskUserQuestion"}"#,
            r#"{"type":"TOOL_CALL_ARGS","toolCallId":"call-2","delta":"{\"questions\":[{\"header\":\"Lang\",\"question\":\"Which?\",\"options\":[{\"label\":\"Go\",\"description\":\"\"}],\"multiSelect\":false}]}"}"#,
            r#"{"type":"TOOL_CALL_END","toolCallId":"call-2"}"#,
            r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r2","outcome":{"type":"interrupt","interrupts":[{"id":"call-2","reason":"input_required","responseSchema":{"type":"object"}}]}}"#,
        ]);
        assert_eq!(events.len(), 4);
        assert!(
            matches!(&events[1], AgentEvent::ApprovalRequest { tool_name, tool_args, tool_call_id }
            if tool_name == "read_file" && tool_args == "{\"path\":\"/tmp/test\"}" && tool_call_id == "call-1")
        );
        assert!(
            matches!(&events[3], AgentEvent::UserQuestionRequest { tool_call_id, questions }
            if tool_call_id == "call-2" && questions[0]["options"][0]["label"] == "Go")
        );
    }

    #[test]
    fn test_parse_messages_snapshot_skipped() {
        let (events, _) = parse_all(&[r#"{"type":"MESSAGES_SNAPSHOT","messages":[]}"#]);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_screen_recording_state_start_stop_and_run_finished() {
        let on = r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/screenRecording","value":{"active":true,"path":"/tmp/r/2026.mp4","region":{"x":100,"y":80,"width":400,"height":300},"frameWidth":1024,"frameHeight":640}}]}"#;
        let (events, _) = parse_all(&[
            r#"{"type":"STATE_SNAPSHOT","snapshot":{"todos":[],"screenRecording":{"active":false}}}"#,
            on,
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/screenRecording","value":{"active":false}}]}"#,
            r#"{"type":"RUN_FINISHED","result":{"inputTokens":1}}"#,
            on,
            r#"{"type":"RUN_FINISHED","result":{"inputTokens":1}}"#,
        ]);
        assert_eq!(events.len(), 5);
        assert!(matches!(
            &events[0],
            AgentEvent::RecordingStarted { path, area }
                if path == "/tmp/r/2026.mp4"
                    && *area == RecordedArea { x: 100, y: 80, width: 400, height: 300, frame_width: 1024, frame_height: 640 }
        ));
        assert!(matches!(&events[1], AgentEvent::RecordingStopped));
        assert!(matches!(
            &events[2],
            AgentEvent::TokenUsage { input: 1, .. }
        ));
        assert!(matches!(&events[3], AgentEvent::RecordingStarted { .. }));
        assert!(matches!(&events[4], AgentEvent::RecordingStopped));
    }

    #[test]
    fn test_parse_screen_recording_snapshot_mid_recording() {
        let (events, _) = parse_all(&[
            r#"{"type":"STATE_SNAPSHOT","snapshot":{"screenRecording":{"active":true,"path":"/tmp/r.mp4","region":{"x":0,"y":0,"width":10,"height":10},"frameWidth":100,"frameHeight":50}}}"#,
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/todos","value":[]}]}"#,
        ]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::RecordingStarted { path, .. } if path == "/tmp/r.mp4")
        );
    }

    #[test]
    fn test_parse_computer_use_activity() {
        let (events, _) = parse_all(&[
            r#"{"type":"ACTIVITY_SNAPSHOT","messageId":"computer_use:c1","activityType":"computer_use","content":{"toolCallId":"c1","action":"click","x":640.5,"y":380,"screenWidth":1440,"screenHeight":900}}"#,
            r#"{"type":"ACTIVITY_SNAPSHOT","messageId":"computer_use:c2","activityType":"computer_use","content":{"toolCallId":"c2","action":"type","screenWidth":1440,"screenHeight":900}}"#,
            r#"{"type":"ACTIVITY_SNAPSHOT","messageId":"judge:Bash:1","activityType":"judge_verdict","content":{"tool":"Bash"}}"#,
        ]);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            AgentEvent::ComputerUseAction { tool_call_id, action, x: Some(x), y: Some(y), screen_width, screen_height }
                if tool_call_id == "c1" && action == "click" && *x == 640.5 && *y == 380.0 && *screen_width == 1440.0 && *screen_height == 900.0
        ));
        assert!(matches!(
            &events[1],
            AgentEvent::ComputerUseAction { action, x: None, y: None, .. } if action == "type"
        ));
    }

    #[test]
    fn test_parse_custom_agent_status() {
        let (events, _) = parse_all(&[
            r#"{"type":"ACTIVITY_SNAPSHOT","messageId":"agent:browser-agent","activityType":"agent_status","content":{"name":"browser-agent","state":"PullingImage","message":"Pulling image","done":3,"total":10}}"#,
        ]);
        assert!(
            matches!(&events[0], AgentEvent::AgentStatus { name, state, done: 3, total: 10, .. }
            if name == "browser-agent" && state == "PullingImage")
        );
    }

    #[test]
    fn test_parse_tool_progress_maps_only_cancelled() {
        let frames: Vec<String> = [
            "queued",
            "starting",
            "running",
            "saving",
            "completed",
            "failed",
            "cancelled",
        ]
        .iter()
        .map(|status| {
            format!(
                r#"{{"type":"ACTIVITY_SNAPSHOT","messageId":"tool:tc1","activityType":"tool_progress","content":{{"toolCallId":"tc1","toolName":"Bash","status":"{status}","message":""}}}}"#
            )
        })
        .collect();
        let lines: Vec<&str> = frames.iter().map(String::as_str).collect();
        let (events, _) = parse_all(&lines);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolCancelled { tool_call_id } if tool_call_id == "tc1")
        );
    }

    #[test]
    fn test_parse_optimization_status_activity() {
        let (events, _) = parse_all(&[
            r#"{"type":"ACTIVITY_SNAPSHOT","messageId":"optimization","activityType":"optimization_status","content":{"message":"Conversation optimized","originalCount":40,"optimizedCount":12}}"#,
        ]);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AgentEvent::ConversationOptimized { message, original_count: 40, optimized_count: 12 }
                if message == "Conversation optimized"
        ));
    }

    #[test]
    fn test_parse_background_note_and_tasks_state() {
        let (events, _) = parse_all(&[
            r#"{"type":"CUSTOM","name":"queued_message","value":{"content":"[A2A Task Completed: t1]\n\nok"}}"#,
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/backgroundTasks","value":{"running":1,"jobs":[{"id":"t1","kind":"a2a","label":"t1","description":"delay 15s","detail":"http://localhost:8081","status":"running","started_at":"2026-09-15T10:00:00Z"}]}}]}"#,
            r#"{"type":"STATE_DELTA","delta":[{"op":"add","path":"/backgroundTasks","value":{"running":0}}]}"#,
        ]);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], AgentEvent::BackgroundNote { content }
            if content.starts_with("[A2A Task Completed: t1]")));
        assert!(
            matches!(&events[1], AgentEvent::BackgroundTasks { running: 1, jobs }
            if jobs.len() == 1 && jobs[0].kind == "a2a" && jobs[0].description == "delay 15s" && jobs[0].status == "running")
        );
        assert!(
            matches!(&events[2], AgentEvent::BackgroundTasks { running: 0, jobs } if jobs.is_empty())
        );
    }

    #[test]
    fn test_parse_custom_token_usage_without_value_is_raw() {
        let (events, _) = parse_all(&[r#"{"type":"CUSTOM","name":"token_usage"}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_custom_unknown_type_is_raw() {
        let (events, _) =
            parse_all(&[r#"{"type":"CUSTOM","name":"other_event","value":{"key":"val"}}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_raw_line() {
        let (events, _) = parse_all(&["this is not valid json at all"]);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::RawLine { line: l } if l == "this is not valid json at all")
        );
    }

    #[test]
    fn test_parse_unknown_type_is_raw() {
        let (events, _) = parse_all(&[r#"{"type":"UNKNOWN_EVENT","data":"something"}"#]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_empty_line_returns_raw() {
        let (events, _) = parse_all(&[""]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::RawLine { .. }));
    }

    #[test]
    fn test_parse_agui_fixture() {
        let fixture = r#"{"type":"RUN_STARTED","threadId":"session-42","runId":"run-1"}
{"type":"TEXT_MESSAGE_START","role":"assistant","messageId":"msg-1","content":"I'll look that up for you."}
{"type":"TOOL_CALL_START","messageId":"msg-1","toolCallId":"call-1","name":"search"}
{"type":"TOOL_CALL_ARGS","messageId":"msg-1","toolCallId":"call-1","args":"{\"q\":\"test\"}"}
{"type":"TOOL_CALL_END","messageId":"msg-1","toolCallId":"call-1"}
{"type":"TEXT_MESSAGE_END","messageId":"msg-1"}
{"type":"TOOL_CALL_RESULT","toolCallId":"call-1","content":"{\"tool_name\":\"search\",\"success\":true}"}
{"type":"RUN_ERROR","message":"API key not found"}"#;

        let mut p = AgentParser::new(None);
        let events: Vec<AgentEvent> = fixture
            .lines()
            .filter_map(|line| {
                let l = line.trim();
                if l.is_empty() { None } else { p.parse_line(l) }
            })
            .collect();

        assert_eq!(events.len(), 5);
        assert!(matches!(&events[0], AgentEvent::RunStarted));
        // Streamed text emits on TEXT_MESSAGE_START, before the tool call.
        assert!(
            matches!(&events[1], AgentEvent::AssistantMessage { content, tool_calls, .. }
            if content == "I'll look that up for you." && tool_calls.is_empty())
        );
        assert!(
            matches!(&events[2], AgentEvent::AssistantMessage { content, tool_calls, .. }
            if content.is_empty() && tool_calls.len() == 1 && tool_calls[0].name == "search")
        );
        assert!(
            matches!(&events[3], AgentEvent::ToolResult { tool_call_id, .. } if tool_call_id == "call-1")
        );
        assert!(
            matches!(&events[4], AgentEvent::AgentError { message } if message == "API key not found")
        );
        assert_eq!(p.take_session_id(), Some("session-42".into()));
    }
}
