import { agentFace } from "@/lib/agents";
import { cn } from "@/lib/utils";
import type { MarkdownAgent } from "@/lib/tauri";

/** An agent's round face: its picture (GIFs animate), else its emoji, else its initial. */
export function AgentAvatar({
  agent,
  size = 20,
  className,
}: {
  agent: Pick<MarkdownAgent, "name" | "emoji" | "avatar">;
  size?: number;
  className?: string;
}) {
  const face = agentFace(agent);
  const style = { width: size, height: size, fontSize: size * 0.55 };
  if (face.kind === "picture") {
    return (
      <img
        src={face.src}
        alt=""
        aria-hidden
        style={style}
        className={cn("shrink-0 rounded-full object-cover", className)}
      />
    );
  }
  return (
    <span
      aria-hidden
      style={style}
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-full bg-primary/15 leading-none",
        face.kind === "initial" && "font-semibold text-primary",
        className,
      )}
    >
      {face.kind === "emoji" ? face.emoji : face.letter}
    </span>
  );
}
