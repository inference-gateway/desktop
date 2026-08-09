import { createRoot } from "react-dom/client";
import App from "./App";
import "./index.css";

// Follow the OS light/dark preference (shadcn tokens gate on the `.dark` class).
const mq = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = () => document.documentElement.classList.toggle("dark", mq.matches);
applyTheme();
mq.addEventListener("change", applyTheme);

const el = document.getElementById("app");
if (el) createRoot(el).render(<App />);
