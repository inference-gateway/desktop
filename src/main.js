// ponytail: no framework, no bundler — plain JS is enough for a chat shell
// Add framework when the UI needs routing, state management, or component composition

const transcript = document.getElementById("chat-transcript");
const promptInput = document.getElementById("prompt-input");
const sendBtn = document.getElementById("send-btn");

sendBtn.addEventListener("click", () => {
  const text = promptInput.value.trim();
  if (!text) return;

  const msg = document.createElement("div");
  msg.textContent = text;
  transcript.appendChild(msg);
  promptInput.value = "";
  transcript.scrollTop = transcript.scrollHeight;
});

promptInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendBtn.click();
  }
});
