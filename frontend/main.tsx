import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import Monitor from "./monitor";
import Overlay from "./overlay";
import "./index.css";

// Follow the OS light/dark preference (shadcn tokens gate on the `.dark` class).
const mq = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = () => document.documentElement.classList.toggle("dark", mq.matches);
applyTheme();
mq.addEventListener("change", applyTheme);

// One entry for every window: the Tauri window label picks the component, so
// multi-page asset resolution can never fall back to the wrong page.
const roots: Record<string, () => React.JSX.Element> = {
  monitor: () => <Monitor />,
  overlay: () => <Overlay />,
};

// Tauri's native drag-drop is off (HTML5 drag is used for chat sorting), so a
// file dropped outside a drop target would make the webview navigate to it.
for (const type of ["dragover", "drop"] as const) {
  document.addEventListener(type, (e) => e.preventDefault());
}

const el = document.getElementById("app");
const label = getCurrentWindow().label;
if (label === "monitor" || label === "overlay") {
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
}
if (el) createRoot(el).render((roots[label] ?? (() => <App />))());
