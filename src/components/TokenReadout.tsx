import { useDesktop } from "@/store";

const fmt = (n: number) => n.toLocaleString();

export function TokenReadout() {
  const { tokenUsage, sessionId } = useDesktop();
  if (!sessionId) return null;
  if (tokenUsage.input === 0 && tokenUsage.output === 0) return null;
  return (
    <div className="mx-auto mb-1 flex max-w-[52rem] items-center justify-end gap-1 px-1 text-[0.7rem] text-muted-foreground">
      <span>in: {fmt(tokenUsage.input)}</span>
      <span className="opacity-40">&middot;</span>
      <span>out: {fmt(tokenUsage.output)}</span>
      {tokenUsage.cached_read > 0 && (
        <>
          <span className="opacity-40">&middot;</span>
          <span>cached: {fmt(tokenUsage.cached_read)}</span>
        </>
      )}
      {tokenUsage.total_tool_calls > 0 && (
        <>
          <span className="opacity-40">&middot;</span>
          <span>tools: {fmt(tokenUsage.total_tool_calls)}</span>
        </>
      )}
    </div>
  );
}
