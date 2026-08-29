import { RotateCcw } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { DEFAULT_SNIPPETS, type Snippet } from "@/lib/snippets";
import { useState } from "react";

function SnippetChip({
  snippet,
  disabled,
  onInsert,
  onReset,
  isModified,
}: {
  snippet: Snippet;
  disabled: boolean;
  onInsert: (prompt: string) => void;
  onReset: (id: string) => void;
  isModified: boolean;
}) {
  const [hovering, setHovering] = useState(false);

  return (
    <span
      role="button"
      tabIndex={disabled ? -1 : 0}
      aria-label={`Insert snippet: ${snippet.label}`}
      className={cn(
        "inline-flex cursor-pointer items-center gap-1 rounded-full border border-border-strong px-3 py-1 text-[0.78rem] font-medium text-muted-foreground transition-colors",
        disabled
          ? "cursor-default opacity-50 pointer-events-none"
          : "hover:bg-secondary hover:text-foreground active:bg-border",
      )}
      onClick={() => onInsert(snippet.prompt)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onInsert(snippet.prompt);
        }
      }}
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
    >
      {snippet.label}
      {isModified && hovering && (
        <button
          aria-label={`Reset ${snippet.label} to default`}
          className="ml-0.5 rounded-full p-0.5 text-muted-foreground hover:text-foreground"
          onClick={(e) => {
            e.stopPropagation();
            onReset(snippet.id);
          }}
        >
          <RotateCcw size={12} />
        </button>
      )}
    </span>
  );
}

export function SnippetBar() {
  const { snippets, running, insertSnippet, resetSnippet, resetAllSnippets } = useDesktop();

  const hasModifications = snippets.some((s) => {
    const def = DEFAULT_SNIPPETS.find((d) => d.id === s.id);
    return def && def.prompt !== s.prompt;
  });

  return (
    <div className="mx-auto mt-2 flex max-w-[52rem] flex-wrap items-center gap-1.5 px-1">
      {snippets.map((snippet) => (
        <SnippetChip
          key={snippet.id}
          snippet={snippet}
          disabled={running}
          onInsert={insertSnippet}
          onReset={resetSnippet}
          isModified={!!DEFAULT_SNIPPETS.find((d) => d.id === snippet.id && d.prompt !== snippet.prompt)}
        />
      ))}
      {hasModifications && (
        <button
          onClick={resetAllSnippets}
          className="ml-auto px-2 py-1 text-[0.7rem] text-muted-foreground underline decoration-dotted underline-offset-2 hover:text-foreground"
          aria-label="Reset all snippets to defaults"
        >
          Reset all
        </button>
      )}
    </div>
  );
}
