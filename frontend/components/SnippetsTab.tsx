import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSnippets } from "@/stores/snippets";
import { DEFAULT_SNIPPETS } from "@/lib/snippets";

export function SnippetsTab() {
  const { snippets, updateSnippet, resetSnippet, resetAllSnippets } = useSnippets();
  const hasModifications = snippets.some((s) => {
    const def = DEFAULT_SNIPPETS.find((d) => d.id === s.id);
    return def && def.prompt !== s.prompt;
  });

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Snippets</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Quick-prompt templates shown below the composer. Changes save automatically.
      </p>
      <div className="flex flex-col gap-4">
        {snippets.map((s) => (
          <div key={s.id} className="rounded-lg border border-border bg-card p-4">
            <div className="mb-3 flex items-center justify-between">
              <Label htmlFor={`snippet-label-${s.id}`} className="text-[0.8rem] font-medium">
                {s.id}
              </Label>
              <Button variant="outline" size="xs" onClick={() => resetSnippet(s.id)}>
                Reset
              </Button>
            </div>
            <div className="mb-2 flex flex-col gap-1">
              <Label htmlFor={`snippet-label-${s.id}`} className="text-[0.75rem] text-muted-foreground">
                Label
              </Label>
              <Input
                id={`snippet-label-${s.id}`}
                value={s.label}
                onChange={(e) => updateSnippet(s.id, { label: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1">
              <Label htmlFor={`snippet-prompt-${s.id}`} className="text-[0.75rem] text-muted-foreground">
                Prompt
              </Label>
              <textarea
                id={`snippet-prompt-${s.id}`}
                rows={3}
                value={s.prompt}
                onChange={(e) => updateSnippet(s.id, { prompt: e.target.value })}
                className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              />
            </div>
          </div>
        ))}
      </div>
      {hasModifications && (
        <div className="mt-4">
          <Button variant="outline" size="sm" onClick={resetAllSnippets}>
            Reset all to defaults
          </Button>
        </div>
      )}
    </>
  );
}
