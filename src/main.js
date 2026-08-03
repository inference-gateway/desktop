// ponytail: no framework, no bundler — plain JS is enough for a chat shell

const statusBar = document.getElementById("status-bar");
const transcript = document.getElementById("chat-transcript");
const promptInput = document.getElementById("prompt-input");
const sendBtn = document.getElementById("send-btn");
const cancelBtn = document.getElementById("cancel-btn");
const modelSelect = document.getElementById("model-select");
const refreshBtn = document.getElementById("refresh-btn");
const chatList = document.getElementById("chat-list");
const newChatBtn = document.getElementById("new-chat-btn");
const settingsBtn = document.getElementById("settings-btn");
const settingsOverlay = document.getElementById("settings-overlay");
const settingsFields = document.getElementById("settings-fields");
const settingsSave = document.getElementById("settings-save");
const settingsCancel = document.getElementById("settings-cancel");

const STORAGE_KEY = "selectedModel";
const MAX_RETRIES = 10;

const PROVIDERS = [
  { label: "OpenAI", env: "OPENAI_API_KEY" },
  { label: "Anthropic", env: "ANTHROPIC_API_KEY" },
  { label: "DeepSeek", env: "DEEPSEEK_API_KEY" },
  { label: "Google", env: "GOOGLE_API_KEY" },
  { label: "Groq", env: "GROQ_API_KEY" },
  { label: "Mistral", env: "MISTRAL_API_KEY" },
  { label: "Cohere", env: "COHERE_API_KEY" },
  { label: "Cloudflare", env: "CLOUDFLARE_API_KEY" },
  { label: "NVIDIA", env: "NVIDIA_API_KEY" },
  { label: "Moonshot", env: "MOONSHOT_API_KEY" },
  { label: "MiniMax", env: "MINIMAX_API_KEY" },
  { label: "Ollama Cloud", env: "OLLAMA_CLOUD_API_KEY" },
];

let activeSessionId = null;
let running = false;
let currentAssistantBubble = null;
let rawLines = [];
let pendingApprovals = [];

function setInputsEnabled(enabled) {
  promptInput.disabled = !enabled;
  sendBtn.disabled = !enabled;
  modelSelect.disabled = !enabled;
  newChatBtn.disabled = !enabled;
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

function addReasoning(text) {
  const bubble = getOrCreateAssistantBubble();
  let details = bubble.previousElementSibling;
  if (!details || !details.classList.contains("reasoning")) {
    details = document.createElement("details");
    details.className = "reasoning";
    const summary = document.createElement("summary");
    summary.textContent = "Thought process";
    details.appendChild(summary);
    transcript.insertBefore(details, bubble);
  }
  const p = document.createElement("p");
  p.textContent = text;
  details.appendChild(p);
  transcript.scrollTop = transcript.scrollHeight;
}

function prettyJson(str) {
  try {
    return JSON.stringify(JSON.parse(str), null, 2);
  } catch {
    return str;
  }
}

function addToolCall(name, argsSummary, output, failed) {
  const details = document.createElement("details");
  details.className = failed ? "tool-call failed" : "tool-call";

  const summary = document.createElement("summary");
  const nameEl = document.createElement("span");
  nameEl.className = "tool-name";
  nameEl.textContent = name;
  const argsEl = document.createElement("span");
  argsEl.className = "tool-args";
  argsEl.textContent = argsSummary;
  summary.append(nameEl, argsEl);

  const pre = document.createElement("pre");
  pre.textContent = [prettyJson(argsSummary), output].filter(Boolean).join("\n\n");

  details.append(summary, pre);
  transcript.appendChild(details);
  transcript.scrollTop = transcript.scrollHeight;
}

// ponytail: infer writes tool results as "Result of tool call: {json}" or a
// plain "Tool execution failed: ..." line; anything unparseable is shown raw.
function addToolResult(content) {
  const brace = content.indexOf("{");
  if (brace !== -1) {
    try {
      const result = JSON.parse(content.slice(brace));
      const data = result.data;
      const output = data?.output ?? (data ? JSON.stringify(data, null, 2) : "");
      addToolCall(
        result.tool_name || "tool",
        JSON.stringify(result.arguments ?? {}),
        output,
        result.success === false,
      );
      return;
    } catch {}
  }
  addToolCall("tool", content, "", /fail|error/i.test(content));
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

async function fetchModelsWithRetry(attempt = 0) {
  try {
    const { invoke } = window.__TAURI__.core;
    const models = await invoke("list_models");
    populateModels(models);
  } catch (err) {
    if (attempt < MAX_RETRIES) {
      setTimeout(() => fetchModelsWithRetry(attempt + 1), 1500);
    }
  }
}

function populateModels(models) {
  modelSelect.innerHTML = "";
  if (!models || models.length === 0) {
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = "No models available";
    opt.disabled = true;
    opt.selected = true;
    modelSelect.appendChild(opt);
    return;
  }
  const saved = localStorage.getItem(STORAGE_KEY);
  for (const id of models) {
    const opt = document.createElement("option");
    opt.value = id;
    opt.textContent = id;
    if (id === saved) opt.selected = true;
    modelSelect.appendChild(opt);
  }
  modelSelect.disabled = running;
}

async function startGatewayThenModels() {
  setStatus("Starting gateway...");
  try {
    await window.__TAURI__.core.invoke("start_gateway");
  } catch (err) {
    console.error("start_gateway failed:", err);
  }
  setStatus("Ready");
  fetchModelsWithRetry();
}

async function refreshChatList() {
  try {
    const { invoke } = window.__TAURI__.core;
    const json = await invoke("list_conversations");
    const data = JSON.parse(json);
    renderChatList(data.conversations || []);
  } catch (err) {
    console.error("Failed to load conversations:", err);
  }
}

function renderChatList(conversations) {
  chatList.innerHTML = "";
  for (const c of conversations) {
    const item = document.createElement("div");
    item.className = "chat-item";
    item.dataset.id = c.id;
    item.textContent = c.title || "(untitled)";
    item.title = c.title || c.id;
    item.addEventListener("click", () => openConversation(c.id));
    chatList.appendChild(item);
  }
  highlightActive();
}

function highlightActive() {
  for (const item of chatList.children) {
    item.classList.toggle("active", item.dataset.id === activeSessionId);
  }
}

async function openConversation(id) {
  if (running) return;
  try {
    const { invoke } = window.__TAURI__.core;
    const ndjson = await invoke("get_conversation", { sessionId: id });
    activeSessionId = id;
    renderTranscript(ndjson);
    highlightActive();
  } catch (err) {
    addError(`Failed to load conversation: ${err}`);
  }
}

function renderTranscript(ndjson) {
  transcript.innerHTML = "";
  currentAssistantBubble = null;
  rawLines = [];
  for (const line of ndjson.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let entry;
    try {
      entry = JSON.parse(trimmed);
    } catch {
      continue;
    }
    const content = entry.content || "";
    if (entry.role === "user") {
      addUserBubble(content);
    } else if (entry.role === "assistant") {
      if (entry.reasoning_content) addReasoning(entry.reasoning_content);
      if (content) appendAssistantContent(content);
      currentAssistantBubble = null;
    } else if (entry.role === "tool") {
      addToolResult(content);
    }
  }
  transcript.scrollTop = transcript.scrollHeight;
}

function startNewChat() {
  if (running) return;
  activeSessionId = null;
  transcript.innerHTML = "";
  currentAssistantBubble = null;
  rawLines = [];
  highlightActive();
  promptInput.focus();
}

let settingsBuilt = false;
function buildSettingsFields() {
  if (settingsBuilt) return;
  for (const p of PROVIDERS) {
    const row = document.createElement("label");
    row.className = "settings-field";
    const span = document.createElement("span");
    span.textContent = p.label;
    const input = document.createElement("input");
    input.type = "password";
    input.autocomplete = "off";
    input.dataset.env = p.env;
    input.placeholder = p.env;
    row.appendChild(span);
    row.appendChild(input);
    settingsFields.appendChild(row);
  }
  settingsBuilt = true;
}

async function openSettings() {
  buildSettingsFields();
  try {
    const { invoke } = window.__TAURI__.core;
    const auth = await invoke("get_auth");
    for (const input of settingsFields.querySelectorAll("input")) {
      input.value = (auth && auth[input.dataset.env]) || "";
    }
  } catch (err) {
    console.error("Failed to load auth:", err);
  }
  settingsOverlay.hidden = false;
}

async function saveSettings() {
  const keys = {};
  for (const input of settingsFields.querySelectorAll("input")) {
    keys[input.dataset.env] = input.value.trim();
  }
  try {
    const { invoke } = window.__TAURI__.core;
    await invoke("set_auth", { keys });
    settingsOverlay.hidden = true;
    startGatewayThenModels();
  } catch (err) {
    addError(`Failed to save settings: ${err}`);
  }
}

async function initBackend() {
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
          startGatewayThenModels();
          refreshChatList();
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
}

initBackend();

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
          activeSessionId = event.session_id;
          break;
        case "AssistantMessage":
          if (event.reasoning_content) {
            addReasoning(event.reasoning_content);
          }
          if (event.content) {
            appendAssistantContent(event.content);
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
      sessionId: activeSessionId,
      onEvent: agentChannel,
    });

    if (newSessionId) {
      activeSessionId = newSessionId;
    }
    refreshChatList();
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

newChatBtn.addEventListener("click", startNewChat);

refreshBtn.addEventListener("click", async () => {
  if (running) {
    try {
      await window.__TAURI__.core.invoke("cancel_agent");
    } catch (err) {
      console.error("Cancel failed:", err);
    }
  }
  setStatus("Restarting CLI...");
  setInputsEnabled(false);
  initBackend();
});

modelSelect.addEventListener("change", () => {
  localStorage.setItem(STORAGE_KEY, modelSelect.value);
});

settingsBtn.addEventListener("click", openSettings);
settingsCancel.addEventListener("click", () => {
  settingsOverlay.hidden = true;
});
settingsSave.addEventListener("click", saveSettings);
settingsOverlay.addEventListener("click", (e) => {
  if (e.target === settingsOverlay) settingsOverlay.hidden = true;
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !settingsOverlay.hidden) settingsOverlay.hidden = true;
});
