// ponytail: no framework, no bundler — plain JS is enough for a chat shell

const statusBar = document.getElementById("status-bar");
const transcript = document.getElementById("chat-transcript");
const promptInput = document.getElementById("prompt-input");
const sendBtn = document.getElementById("send-btn");
const cancelBtn = document.getElementById("cancel-btn");
const modelSelect = document.getElementById("model-select");
const retryBtn = document.getElementById("retry-btn");

const STORAGE_KEY = "selectedModel";
const MAX_RETRIES = 10;

let sessionId = null;
let running = false;
let currentAssistantBubble = null;
let rawLines = [];
let pendingApprovals = [];

function setInputsEnabled(enabled) {
  promptInput.disabled = !enabled;
  sendBtn.disabled = !enabled;
  modelSelect.disabled = !enabled;
  if (enabled) {
    cancelBtn.style.display = "none";
  }
}

function setRunning(val) {
  running = val;
  setInputsEnabled(!val);
  if (val) {
    cancelBtn.style.display = "";
  }
}

function setStatus(text) {
  statusBar.textContent = text;
  statusBar.className = "";
}

function setError(text) {
  statusBar.textContent = text;
  statusBar.className = "error";
}

function addUserBubble(text) {
  const div = document.createElement("div");
  div.className = "bubble user-bubble";
  div.textContent = text;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function getOrCreateAssistantBubble() {
  if (!currentAssistantBubble || !document.body.contains(currentAssistantBubble)) {
    const div = document.createElement("div");
    div.className = "bubble assistant-bubble";
    transcript.appendChild(div);
    currentAssistantBubble = div;
  }
  return currentAssistantBubble;
}

function appendAssistantContent(text) {
  const bubble = getOrCreateAssistantBubble();
  const p = document.createElement("p");
  p.textContent = text;
  bubble.appendChild(p);
  transcript.scrollTop = transcript.scrollHeight;
}

function addToolCall(name, argsSummary) {
  const div = document.createElement("div");
  div.className = "tool-call";
  div.innerHTML = `<span class="tool-name">${escapeHtml(name)}</span> <span class="tool-args">${escapeHtml(argsSummary)}</span>`;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function addInfo(message) {
  const div = document.createElement("div");
  div.className = "info-line";
  div.textContent = message;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function addWarning(message) {
  const div = document.createElement("div");
  div.className = "warning-line";
  div.textContent = message;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function addError(message) {
  const div = document.createElement("div");
  div.className = "error-line";
  div.textContent = message;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function addRawLine(line) {
  rawLines.push(line);
  updateRawOutput();
}

function updateRawOutput() {
  let container = document.getElementById("raw-output");
  if (!container && rawLines.length > 0) {
    container = document.createElement("div");
    container.id = "raw-output";
    container.className = "raw-output";
    const summary = document.createElement("div");
    summary.className = "raw-summary";
    summary.textContent = `Raw output (${rawLines.length} lines)`;
    summary.addEventListener("click", () => {
      const body = container.querySelector(".raw-body");
      body.style.display = body.style.display === "none" ? "" : "none";
    });
    container.appendChild(summary);
    const body = document.createElement("pre");
    body.className = "raw-body";
    body.style.display = "none";
    container.appendChild(body);
    transcript.appendChild(container);
  }
  if (container) {
    const body = container.querySelector(".raw-body");
    body.textContent = rawLines.join("\n");
  }
  transcript.scrollTop = transcript.scrollHeight;
}

function addCancelled() {
  const div = document.createElement("div");
  div.className = "cancelled-line";
  div.textContent = "Cancelled";
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function escapeHtml(str) {
  const div = document.createElement("div");
  div.textContent = str;
  return div.innerHTML;
}

function addApprovalPrompt(toolName, toolArgs, toolCallId) {
  const div = document.createElement("div");
  div.className = "approval-prompt";
  div.id = `approval-${toolCallId}`;
  div.innerHTML = `
    <div class="approval-header">Tool requires approval</div>
    <div class="approval-tool"><span class="tool-name">${escapeHtml(toolName)}</span></div>
    <pre class="approval-args">${escapeHtml(toolArgs)}</pre>
    <div class="approval-buttons">
      <button class="approve-btn" data-call-id="${escapeHtml(toolCallId)}">Approve</button>
      <button class="deny-btn" data-call-id="${escapeHtml(toolCallId)}">Deny</button>
    </div>
  `;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;

  pendingApprovals.push(toolCallId);

  div.querySelector(".approve-btn").addEventListener("click", () => sendApproval(toolCallId, true));
  div.querySelector(".deny-btn").addEventListener("click", () => sendApproval(toolCallId, false));
}

async function sendApproval(toolCallId, approved) {
  try {
    const { invoke } = window.__TAURI__.core;
    await invoke("send_approval", { toolCallId, approved });
    const prompt = document.getElementById(`approval-${toolCallId}`);
    if (prompt) {
      prompt.classList.add(approved ? "approved" : "denied");
      const btns = prompt.querySelector(".approval-buttons");
      if (btns) btns.style.display = "none";
      const label = document.createElement("div");
      label.className = "approval-result";
      label.textContent = approved ? "Approved" : "Denied";
      prompt.appendChild(label);
    }
    pendingApprovals = pendingApprovals.filter(id => id !== toolCallId);
  } catch (err) {
    addError(`Approval failed: ${err}`);
  }
}

(async () => {
  try {
    const { invoke, Channel } = window.__TAURI__.core;

    const channel = new Channel();
    channel.onmessage = (event) => {
      switch (event.kind) {
        case "Checking":
          setStatus("Checking for infer binary...");
          break;
        case "Downloading":
          if (event.total > 0) {
            const pct = Math.round((event.received / event.total) * 100);
            setStatus(`Downloading infer... ${pct}%`);
          } else {
            setStatus("Downloading infer...");
          }
          break;
        case "Verifying":
          setStatus("Verifying download...");
          break;
        case "Installing":
          setStatus("Installing infer...");
          break;
        case "Initializing":
          setStatus("Running initial setup...");
          break;
        case "Ready":
          setStatus("Ready");
          setInputsEnabled(true);
          fetchModelsWithRetry();
          break;
        case "Error":
          setError(`Error: ${event.message}`);
          break;
      }
    };

    await invoke("check_and_install_cli", { onEvent: channel });
  } catch (err) {
    setError(`Setup failed: ${err}`);
  }
})();

sendBtn.addEventListener("click", async () => {
  const text = promptInput.value.trim();
  if (!text || running) return;

  const model = modelSelect.value;
  if (!model) {
    setError("Please select a model first");
    return;
  }

  setRunning(true);
  setStatus("Running...");
  rawLines = [];

  addUserBubble(text);
  promptInput.value = "";

  try {
    const { invoke, Channel } = window.__TAURI__.core;

    const agentChannel = new Channel();
    agentChannel.onmessage = (event) => {
      switch (event.kind) {
        case "SessionId":
          sessionId = event.session_id;
          break;
        case "AssistantMessage":
          if (event.content) {
            appendAssistantContent(event.content);
          }
          if (event.reasoning_content) {
            const bubble = getOrCreateAssistantBubble();
            const p = document.createElement("p");
            p.className = "reasoning";
            p.textContent = event.reasoning_content;
            bubble.appendChild(p);
          }
          break;
        case "ToolCall":
          addToolCall(event.name, event.args_summary);
          break;
        case "ApprovalRequest":
          addApprovalPrompt(event.tool_name, event.tool_args, event.tool_call_id);
          setStatus("Awaiting approval...");
          break;
        case "Info":
          addInfo(event.message);
          break;
        case "Warning":
          addWarning(event.message);
          break;
        case "AgentError":
          addError(event.message);
          break;
        case "RawLine":
          addRawLine(event.line);
          break;
        case "Done":
          setRunning(false);
          if (event.exit_code === 0) {
            setStatus("Done");
          } else {
            setStatus(`Exited with code ${event.exit_code}`);
          }
          currentAssistantBubble = null;
          break;
        case "Cancelled":
          addCancelled();
          setRunning(false);
          setStatus("Cancelled");
          currentAssistantBubble = null;
          break;
      }
    };

    const newSessionId = await invoke("send_message", {
      prompt: text,
      model: model,
      sessionId: sessionId,
      onEvent: agentChannel,
    });

    if (newSessionId) {
      sessionId = newSessionId;
    }
  } catch (err) {
    addError(`Error: ${err}`);
    setRunning(false);
    setStatus("Error");
    currentAssistantBubble = null;
  }
});

cancelBtn.addEventListener("click", async () => {
  if (!running) return;
  try {
    const { invoke } = window.__TAURI__.core;
    await invoke("cancel_agent");
  } catch (err) {
    console.error("Cancel failed:", err);
  }
});

promptInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendBtn.click();
  }
});
