import { ChevronDown } from "lucide-react";
import { useDesktop } from "@/store";
import { useAgents } from "@/stores/agents";
import { useBackend } from "@/stores/backend";
import { agentPinsModel, agentThreadModel } from "@/lib/agents";

// Native <select> (AXPopUpButton) - required by the e2e harness; do not replace
// with a custom dropdown. It is restyled as a chip for the bar above the composer.
export function ModelSelect() {
  const { models, model, setModel } = useBackend();
  const { agents } = useAgents();
  const { enabled, currentAgent } = useDesktop();
  const agent = agents.find((a) => a.name === currentAgent);
  const shown = agentThreadModel(agent, model);
  const ownModel = agentPinsModel(agent);
  const options = models.includes(shown) || !shown ? models : [shown, ...models];
  return (
    <div className="relative min-w-0 shrink">
      <select
        id="model-select"
        aria-label="Model"
        value={shown}
        disabled={!enabled || ownModel}
        title={ownModel ? `${agent?.name} runs on its own model - change it with Edit agent` : shown}
        onChange={(e) => setModel(e.target.value)}
        className="field-sizing-content max-w-[16rem] cursor-pointer appearance-none truncate rounded-full bg-transparent py-0.5 pl-2.5 pr-6 text-[0.8rem] text-muted-foreground transition-colors hover:bg-card hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:cursor-default disabled:hover:bg-transparent disabled:hover:text-muted-foreground"
      >
        {options.length === 0 ? (
          <option value="" disabled className="bg-card text-foreground">
            Waiting for gateway...
          </option>
        ) : (
          options.map((m) => (
            <option key={m} value={m} className="bg-card text-foreground">
              {m}
            </option>
          ))
        )}
      </select>
      <ChevronDown
        size={12}
        aria-hidden="true"
        className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground"
      />
    </div>
  );
}
