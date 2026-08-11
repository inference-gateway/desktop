import { DesktopProvider } from "@/store";
import { TopBar } from "@/components/TopBar";
import { Main } from "@/components/Main";

// Renders directly into #app as <header id="top-bar"> + <div id="main"> - no
// wrapper element, to preserve the shallow DOM the e2e accessibility selectors
// depend on.
export default function App() {
  return (
    <DesktopProvider>
      <TopBar />
      <Main />
    </DesktopProvider>
  );
}
