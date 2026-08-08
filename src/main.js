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
const updateBtn = document.getElementById("update-btn");
const versionBadge = document.getElementById("version-badge");
const versionList = document.getElementById("version-list");
const checkUpdatesBtn = document.getElementById("check-updates-btn");
const installUpdateBtn = document.getElementById("install-update-btn");
const micBtn = document.getElementById("mic-btn");

const STORAGE_KEY = "selectedModel";
const UPDATE_CACHE_KEY = "updateCheck";
const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;
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

const md = window.markdownit({ linkify: true });

let activeSessionId = null;
let running = false;
let currentAssistantBubble = null;
let pendingApprovals = [];
let desktopOutdated = false;
const pendingToolCalls = new Map();

function setInputsEnabled(enabled) {
  promptInput.disabled = !enabled;
  sendBtn.disabled = !enabled;
  modelSelect.disabled = !enabled;
  newChatBtn.disabled = !enabled;
  if (enabled) {
    cancelBtn.style.display = "none";
  }
  updateMicAvailability();
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

// ponytail: markdown-it already handles ![alt](url), so we just normalize bare
// image URLs/data URIs to markdown image syntax before rendering. The onerror
// handler replaces broken images with a fallback message.
function normalizeImageUrls(text) {
  return text.replace(
    /(?<!\]\()(data:image\/[a-z]+(?:;[a-z0-9-]+)*;base64,[A-Za-z0-9+/=]+|https?:\/\/[^\s)]+\.(?:png|jpg|jpeg|gif|webp|svg|avif|bmp)(?:\?[^\s)]*)?)/gi,
    (url) => `![](${url})`
  );
}

// ponytail: each AssistantMessage event renders as its own markdown doc (one
// <p> per chunk). Ceiling: a code fence split across two stream events renders
// wrong — accumulate full text before parsing only if that shows up.
function appendAssistantContent(text) {
  hideTyping();
  const bubble = getOrCreateAssistantBubble();
  const p = document.createElement("p");
  p.innerHTML = md.render(normalizeImageUrls(text));
  for (const img of p.querySelectorAll("img")) {
    img.onerror = () => {
      img.outerHTML = '<span class="image-error">Image failed to load</span>';
    };
  }
  bubble.appendChild(p);
  transcript.scrollTop = transcript.scrollHeight;
}

function showTyping() {
  if (document.getElementById("typing-indicator")) return;
  const div = document.createElement("div");
  div.className = "bubble assistant-bubble typing";
  div.id = "typing-indicator";
  div.innerHTML = "<span></span><span></span><span></span>";
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function hideTyping() {
  document.getElementById("typing-indicator")?.remove();
}

function showImageSkeleton() {
  const div = document.createElement("div");
  div.className = "image-skeleton";
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
  return div;
}

// ponytail: infer's ImageGeneration result carries the saved absolute path
// (data.path); WKWebView can't load a bare file path, so serve it through
// Tauri's asset protocol. Deduped by filename so ImageGeneration + ImageDecode
// (which re-reports the same file as data.source) don't render it twice.
function renderResultImage(r) {
  const p = r && r.imagePath;
  if (!p || !/\.infer\/tmp\/[\w.-]+\.(?:png|gif|webp|avif|jpe?g)$/i.test(p)) {
    console.warn("renderResultImage: path did not match safe pattern:", p);
    return;
  }
  const file = p.split("/").pop();
  if (transcript.querySelector(`img[data-infer="${CSS.escape(file)}"]`)) return;
  const img = document.createElement("img");
  img.className = "generated-image";
  img.dataset.infer = file;
  img.src = window.__TAURI__.core.convertFileSrc(p);
  transcript.appendChild(img);
  transcript.scrollTop = transcript.scrollHeight;
}

function addReasoning(text) {
  hideTyping();
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
  hideTyping();
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
  return details;
}

// ponytail: AG-UI TOOL_CALL_RESULT.content is raw JSON; anything
// unparseable is shown raw.
function parseToolResult(content) {
  const brace = content.indexOf("{");
  if (brace === -1) return null;
  try {
    const result = JSON.parse(content.slice(brace));
    const data = result.data;
    const output = data?.output ?? (data ? JSON.stringify(data, null, 2) : "");
    return {
      name: result.tool_name || "tool",
      args: JSON.stringify(result.arguments ?? {}),
      output,
      failed: result.success === false,
      imagePath: data?.path || data?.source || null,
    };
  } catch {
    return null;
  }
}

function addToolResult(content) {
  const r = parseToolResult(content);
  if (r) {
    addToolCall(r.name, r.args, r.output, r.failed);
    renderResultImage(r);
  } else {
    addToolCall("tool", content, "", /fail|error/i.test(content));
  }
}

function resolveToolCall(id, content) {
  const details = pendingToolCalls.get(id);
  pendingToolCalls.delete(id);
  details?._skeleton?.remove(); // ponytail: only set on image tool calls, no-op otherwise
  if (!details || !document.body.contains(details)) {
    addToolResult(content);
    return;
  }
  const r = parseToolResult(content);
  renderResultImage(r);
  const output = r ? r.output : content;
  const failed = r ? r.failed : /fail|error/i.test(content);
  details.classList.remove("running");
  if (failed) details.classList.add("failed");
  const argsSummary = details.querySelector(".tool-args")?.textContent ?? "";
  details.querySelector("pre").textContent = [prettyJson(argsSummary), output]
    .filter(Boolean)
    .join("\n\n");
}

function finishPendingToolCalls() {
  for (const details of pendingToolCalls.values()) {
    details.classList.remove("running");
    details._skeleton?.remove(); // ponytail: only set on image tool calls, no-op otherwise
  }
  pendingToolCalls.clear();
}

function addWarning(message) {
  hideTyping();
  const div = document.createElement("div");
  div.className = "warning-line";
  div.textContent = message;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function addError(message) {
  hideTyping();
  const div = document.createElement("div");
  div.className = "error-line";
  div.textContent = message;
  transcript.appendChild(div);
  transcript.scrollTop = transcript.scrollHeight;
}

function addCancelled() {
  hideTyping();
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
  hideTyping();
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

async function startGatewayThenModels(force = false) {
  setStatus("Starting gateway...");
  try {
    await window.__TAURI__.core.invoke("start_gateway", { force });
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

const TRASH_ICON =
  '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>';

function renderChatList(conversations) {
  chatList.innerHTML = "";
  for (const c of conversations) {
    const item = document.createElement("div");
    item.className = "chat-item";
    item.dataset.id = c.id;
    item.title = c.title || c.id;
    item.addEventListener("click", () => openConversation(c.id));

    const label = document.createElement("span");
    label.className = "chat-item-title";
    label.textContent = c.title || "(untitled)";
    item.appendChild(label);

    const del = document.createElement("button");
    del.className = "chat-item-delete";
    del.title = "Delete conversation";
    del.setAttribute("aria-label", "Delete conversation");
    del.innerHTML = TRASH_ICON;
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      if (!del.classList.contains("confirm")) {
        del.classList.add("confirm");
        del.title = "Click again to delete";
        return;
      }
      deleteConversation(c.id);
    });
    del.addEventListener("mouseleave", () => {
      del.classList.remove("confirm");
      del.title = "Delete conversation";
    });
    item.appendChild(del);

    chatList.appendChild(item);
  }
  highlightActive();
}

async function deleteConversation(id) {
  if (running) return;
  try {
    const { invoke } = window.__TAURI__.core;
    await invoke("delete_conversation", { sessionId: id });
    if (id === activeSessionId) startNewChat();
    refreshChatList();
  } catch (err) {
    addError(`Failed to delete conversation: ${err}`);
  }
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
  checkForUpdates();
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

function renderUpdates(updates) {
  versionBadge.textContent = updates.map((u) => `${u.name} ${u.current}`).join(" · ");
  const outdated = updates.filter((u) => u.outdated);
  desktopOutdated = outdated.some((u) => u.name === "Desktop");
  updateBtn.hidden = outdated.length === 0;
  installUpdateBtn.hidden = outdated.length === 0;
  if (outdated.length > 0) {
    updateBtn.textContent = `${outdated
      .map((u) => `${u.name} ${u.latest}`)
      .join(", ")} available - restart to update`;
  }
  versionList.innerHTML = "";
  for (const u of updates) {
    const row = document.createElement("div");
    row.className = "version-row";
    const latest = u.latest ? (u.outdated ? `→ ${u.latest}` : "up to date") : "unknown";
    row.textContent = `${u.name} ${u.current} ${latest}`;
    versionList.appendChild(row);
  }
}

// GitHub allows 60 API requests/hour unauthenticated, so a cached result is
// reused within a session and the automatic re-check runs at most every 6
// hours. The cache is invalidated on every app launch so a new version is
// detected at startup; the stale data is kept as a fallback for when the
// check itself fails (offline, rate-limited).
async function checkForUpdates(force = false) {
  const cached = JSON.parse(localStorage.getItem(UPDATE_CACHE_KEY) || "null");
  if (!force && cached && Date.now() - cached.checkedAt < UPDATE_INTERVAL_MS) {
    renderUpdates(cached.updates);
    return;
  }
  try {
    const updates = await window.__TAURI__.core.invoke("check_updates");
    localStorage.setItem(UPDATE_CACHE_KEY, JSON.stringify({ checkedAt: Date.now(), updates }));
    renderUpdates(updates);
  } catch (err) {
    console.error("check_updates failed:", err);
    if (cached) renderUpdates(cached.updates);
  }
}

// The CLI and gateway update in place and are respawned by restartBackend; the
// desktop app swaps its own bundle and relaunches the process, so it goes last
// and nothing after install_desktop_update runs when it succeeds.
async function applyUpdates() {
  updateBtn.disabled = true;
  installUpdateBtn.disabled = true;
  await restartBackend(true);
  if (desktopOutdated) {
    setStatus("Updating app...");
    try {
      await window.__TAURI__.core.invoke("install_desktop_update");
    } catch (err) {
      addError(`App update failed: ${err}`);
    }
  }
  updateBtn.disabled = false;
  installUpdateBtn.disabled = false;
}

async function restartBackend(force) {
  if (running) {
    try {
      await window.__TAURI__.core.invoke("cancel_agent");
    } catch (err) {
      console.error("Cancel failed:", err);
    }
  }
  setStatus(force ? "Updating..." : "Restarting CLI...");
  setInputsEnabled(false);
  await initBackend(force);
}

async function initBackend(force = false) {
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
          startGatewayThenModels(force).then(() => checkForUpdates(force));
          refreshChatList();
          break;
        case "Error":
          setError(`Error: ${event.message}`);
          break;
      }
    };

    await invoke("check_and_install_cli", { onEvent: channel, force });
  } catch (err) {
    setError(`Setup failed: ${err}`);
  }
}

const staleUpdateCache = JSON.parse(localStorage.getItem(UPDATE_CACHE_KEY) || "null");
if (staleUpdateCache) {
  localStorage.setItem(
    UPDATE_CACHE_KEY,
    JSON.stringify({ ...staleUpdateCache, checkedAt: 0 })
  );
}

initBackend();
setInterval(() => checkForUpdates(true), UPDATE_INTERVAL_MS);

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

  addUserBubble(text);
  promptInput.value = "";
  autoGrow();

  showTyping();

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
          if (event.tool_calls?.length) {
            for (const tc of event.tool_calls) {
              const details = addToolCall(tc.name, tc.args);
              details.classList.add("running");
              if (/^Image(Generation|Edit|Variation)$/.test(tc.name)) details._skeleton = showImageSkeleton();
              if (tc.id) pendingToolCalls.set(tc.id, details);
            }
            currentAssistantBubble = null;
          }
          break;
        case "ToolResult":
          resolveToolCall(event.tool_call_id, event.content);
          break;
        case "ApprovalRequest":
          addApprovalPrompt(event.tool_name, event.tool_args, event.tool_call_id);
          setStatus("Awaiting approval...");
          break;
        case "Warning":
          addWarning(event.message);
          break;
        case "AgentError":
          addError(event.message);
          break;
        case "Done":
          hideTyping();
          setRunning(false);
          finishPendingToolCalls();
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
          finishPendingToolCalls();
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
    hideTyping();
    addError(`Error: ${err}`);
    setRunning(false);
    finishPendingToolCalls();
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

function autoGrow() {
  promptInput.style.height = "auto";
  promptInput.style.height = `${promptInput.scrollHeight}px`;
}

promptInput.addEventListener("input", autoGrow);

promptInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendBtn.click();
  }
});

// --- Voice-to-text (mic) ---
// Capture in the WebView, resample to 16kHz mono WAV, hand to the Rust
// transcribe_audio command (whisper.cpp). Mic is greyed when whisper is
// unavailable, permission is denied, or the agent is running.

const MAX_REC_MS = 30000;
let sttBinary = false;
let sttModel = false;
let sttDownloadable = false;
let sttHint = "";
let micPermission = "prompt";
let recording = false;
let mediaStream = null;
let audioCtx = null;
let recNode = null;
let recChunks = [];
let recSampleRate = 48000;
let recTimer = null;

function updateMicAvailability() {
  if (!micBtn) return;
  if (recording) {
    micBtn.disabled = false;
    return;
  }
  const denied = micPermission === "denied";
  const ready = sttBinary && sttModel;
  const canPrepare = sttBinary || sttDownloadable;
  micBtn.disabled = running || denied || !(ready || canPrepare);
  micBtn.title = running
    ? "Wait for the current message to finish"
    : denied
      ? "Microphone access denied - enable it in System Settings"
      : !canPrepare
        ? sttHint || "Voice input unavailable"
        : "Voice input";
}

async function refreshSttStatus() {
  try {
    const { invoke } = window.__TAURI__.core;
    const s = await invoke("stt_status");
    sttBinary = s.binary;
    sttModel = s.model;
    sttDownloadable = s.downloadable;
    sttHint = s.hint;
  } catch (err) {
    sttBinary = sttModel = false;
  }
  updateMicAvailability();
}

async function refreshMicPermission() {
  if (!navigator.permissions?.query) return;
  try {
    const st = await navigator.permissions.query({ name: "microphone" });
    micPermission = st.state;
    st.onchange = () => {
      micPermission = st.state;
      updateMicAvailability();
    };
  } catch (err) {
    // WebKit may not expose the microphone permission - fall back to prompt-on-use
  }
  updateMicAvailability();
}

async function ensureStt() {
  const { invoke, Channel } = window.__TAURI__.core;
  const ch = new Channel();
  ch.onmessage = (e) => {
    switch (e.kind) {
      case "Checking":
        setStatus("Preparing voice input...");
        break;
      case "Installing":
        setStatus("Installing whisper...");
        break;
      case "Downloading":
        setStatus(
          e.total > 0
            ? `Downloading voice model... ${Math.round((e.received / e.total) * 100)}%`
            : "Downloading voice model..."
        );
        break;
      case "Verifying":
        setStatus("Verifying...");
        break;
    }
  };
  await invoke("prepare_stt", { onEvent: ch });
}

function mergeChunks(chunks) {
  let len = 0;
  for (const c of chunks) len += c.length;
  const out = new Float32Array(len);
  let off = 0;
  for (const c of chunks) {
    out.set(c, off);
    off += c.length;
  }
  return out;
}

function downsample(samples, from, to) {
  if (to >= from) return samples;
  const ratio = from / to;
  const outLen = Math.floor(samples.length / ratio);
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const idx = i * ratio;
    const lo = Math.floor(idx);
    const hi = Math.min(lo + 1, samples.length - 1);
    const frac = idx - lo;
    out[i] = samples[lo] * (1 - frac) + samples[hi] * frac;
  }
  return out;
}

function encodeWav(samples, sampleRate) {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const writeStr = (off, s) => {
    for (let i = 0; i < s.length; i++) view.setUint8(off + i, s.charCodeAt(i));
  };
  writeStr(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeStr(36, "data");
  view.setUint32(40, samples.length * 2, true);
  let off = 44;
  for (let i = 0; i < samples.length; i++, off += 2) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(off, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Uint8Array(buffer);
}

async function startRecording() {
  try {
    mediaStream = await navigator.mediaDevices.getUserMedia({ audio: true });
  } catch (err) {
    micPermission = "denied";
    updateMicAvailability();
    setError("Microphone access denied");
    return;
  }
  micPermission = "granted";
  audioCtx = new (window.AudioContext || window.webkitAudioContext)();
  recSampleRate = audioCtx.sampleRate;
  recChunks = [];
  const source = audioCtx.createMediaStreamSource(mediaStream);
  recNode = audioCtx.createScriptProcessor(4096, 1, 1);
  recNode.onaudioprocess = (e) => {
    recChunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));
  };
  source.connect(recNode);
  recNode.connect(audioCtx.destination);
  recording = true;
  micBtn.classList.add("recording");
  micBtn.title = "Stop recording";
  setStatus("Recording... click the mic to stop");
  recTimer = setTimeout(stopRecording, MAX_REC_MS);
}

async function stopRecording() {
  if (!recording) return;
  recording = false;
  clearTimeout(recTimer);
  micBtn.classList.remove("recording");
  if (recNode) {
    recNode.disconnect();
    recNode.onaudioprocess = null;
  }
  if (mediaStream) mediaStream.getTracks().forEach((t) => t.stop());
  const ctxRate = recSampleRate;
  if (audioCtx) {
    await audioCtx.close();
    audioCtx = null;
  }
  updateMicAvailability();

  const samples = mergeChunks(recChunks);
  recChunks = [];
  if (samples.length === 0) {
    setStatus("No audio captured");
    return;
  }
  const wav = encodeWav(downsample(samples, ctxRate, 16000), 16000);

  setStatus("Transcribing...");
  micBtn.disabled = true;
  try {
    const { invoke } = window.__TAURI__.core;
    const text = await invoke("transcribe_audio", { wav: Array.from(wav) });
    if (text && text.trim()) {
      promptInput.value = promptInput.value
        ? promptInput.value.trimEnd() + " " + text.trim()
        : text.trim();
      autoGrow();
      promptInput.focus();
      setStatus("Ready");
    } else {
      setStatus("No speech detected");
    }
  } catch (err) {
    setError(`Transcription failed: ${err}`);
  } finally {
    updateMicAvailability();
  }
}

micBtn.addEventListener("click", async () => {
  if (recording) return stopRecording();
  if (running) return;
  if (!(sttBinary && sttModel)) {
    micBtn.disabled = true;
    try {
      await ensureStt();
      await refreshSttStatus();
    } catch (err) {
      setError(`Voice setup failed: ${err}`);
      await refreshSttStatus();
      return;
    }
    if (!(sttBinary && sttModel)) return;
  }
  await startRecording();
});

refreshSttStatus();
refreshMicPermission();

newChatBtn.addEventListener("click", startNewChat);

refreshBtn.addEventListener("click", () => restartBackend(false));

updateBtn.addEventListener("click", applyUpdates);

installUpdateBtn.addEventListener("click", async () => {
  settingsOverlay.hidden = true;
  await applyUpdates();
});

checkUpdatesBtn.addEventListener("click", async () => {
  checkUpdatesBtn.disabled = true;
  checkUpdatesBtn.textContent = "Checking...";
  await checkForUpdates(true);
  checkUpdatesBtn.disabled = false;
  checkUpdatesBtn.textContent = "Check for updates";
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
