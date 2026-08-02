// ponytail: no framework, no bundler — plain JS is enough for a chat shell
// Add framework when the UI needs routing, state management, or component composition

const statusBar = document.getElementById("status-bar");
const transcript = document.getElementById("chat-transcript");
const promptInput = document.getElementById("prompt-input");
const sendBtn = document.getElementById("send-btn");
const modelSelect = document.getElementById("model-select");
const retryBtn = document.getElementById("retry-btn");

const STORAGE_KEY = "selectedModel";
const MAX_RETRIES = 10;

function setInputsEnabled(enabled) {
  promptInput.disabled = !enabled;
  sendBtn.disabled = !enabled;
  // model-select is managed separately by model state
}

function setStatus(text) {
  statusBar.textContent = text;
  statusBar.className = "";
}

function setError(text) {
  statusBar.textContent = text;
  statusBar.className = "error";
}

// --- Model dropdown states ---

function setModelWaiting() {
  modelSelect.innerHTML = '<option value="" disabled selected>Waiting for gateway...</option>';
  modelSelect.disabled = true;
  retryBtn.style.display = "none";
}

function setModelUnreachable(errMsg) {
  modelSelect.innerHTML = '<option value="" disabled selected>Gateway not reachable</option>';
  modelSelect.disabled = true;
  retryBtn.style.display = "inline-block";
  retryBtn.dataset.lastError = errMsg;
}

function populateModels(models) {
  const prev = localStorage.getItem(STORAGE_KEY);
  modelSelect.innerHTML = '<option value="" disabled>Select a model...</option>';
  for (const m of models) {
    const opt = document.createElement("option");
    opt.value = m;
    opt.textContent = m;
    if (m === prev) opt.selected = true;
    modelSelect.appendChild(opt);
  }
  modelSelect.disabled = false;
  retryBtn.style.display = "none";
}

// --- Model fetching with retry ---

let retryCount = 0;
let fetchAborted = false;

async function fetchModelsWithRetry() {
  const { invoke } = window.__TAURI__.core;
  retryCount = 0;
  fetchAborted = false;
  setModelWaiting();

  while (retryCount < MAX_RETRIES && !fetchAborted) {
    try {
      const models = await invoke("fetch_models");
      if (fetchAborted) return;
      populateModels(models);
      setStatus("Ready");
      setInputsEnabled(true);
      return;
    } catch (err) {
      if (fetchAborted) return;
      retryCount++;
      if (retryCount >= MAX_RETRIES) {
        setModelUnreachable(err);
        setError(`Gateway not reachable: ${err}`);
        return;
      }
      // Backoff: 1s, 2s, 4s, 8s, 16s, 30s (capped)
      const delay = Math.min(1000 * Math.pow(2, retryCount - 1), 30000);
      setStatus(`Waiting for gateway (attempt ${retryCount}/${MAX_RETRIES})...`);
      await new Promise(r => setTimeout(r, delay));
    }
  }
}

retryBtn.addEventListener("click", () => {
  setStatus("Retrying...");
  setInputsEnabled(false);
  fetchModelsWithRetry();
});

// --- Persist selected model ---

modelSelect.addEventListener("change", () => {
  const val = modelSelect.value;
  if (val) {
    localStorage.setItem(STORAGE_KEY, val);
  } else {
    localStorage.removeItem(STORAGE_KEY);
  }
});

// --- Send message ---

function addMessage(text, model) {
  const msg = document.createElement("div");
  msg.className = "message";

  const body = document.createElement("div");
  body.className = "message-body";
  body.textContent = text;
  msg.appendChild(body);

  if (model) {
    const badge = document.createElement("span");
    badge.className = "model-badge";
    badge.textContent = model;
    msg.appendChild(badge);
  }

  transcript.appendChild(msg);
  transcript.scrollTop = transcript.scrollHeight;
}

sendBtn.addEventListener("click", () => {
  const text = promptInput.value.trim();
  if (!text) return;

  const model = modelSelect.value || null;
  addMessage(text, model);
  promptInput.value = "";
});

promptInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendBtn.click();
  }
});

// --- Startup ---

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
          // Start fetching models after CLI is ready
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
