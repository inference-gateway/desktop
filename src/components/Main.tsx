import { Sidebar } from "./Sidebar";
import { Transcript } from "./Transcript";
import { Composer } from "./Composer";

export function Main() {
  return (
    <div id="main" className="flex min-h-0 flex-1">
      <Sidebar />
      <div id="content" className="flex min-w-0 flex-1 flex-col">
        <Transcript />
        <Composer />
      </div>
    </div>
  );
}
