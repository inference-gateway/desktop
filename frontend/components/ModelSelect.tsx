import { useDesktop } from "@/store";
import { agentPinsModel, agentThreadModel } from "@/lib/agents";

// Native <select> (AXPopUpButton) - required by the e2e harness; do not replace
// with a custom dropdown.
export function ModelSelect() {
  const { models, model, setModel, enabled, agents, currentAgent } = useDesktop();
  const agent = agents.find((a) => a.name === currentAgent);
  const shown = agentThreadModel(agent, model);
  const options = models.includes(shown) || !shown ? models : [shown, ...models];
  return (
    <select
      id="model-select"
      value={shown}
      disabled={!enabled || agent !== undefined}
      title={
        agent
          ? `${agent.name} runs on ${agentPinsModel(agent) ? "its own model" : "the default model"} - change it with Edit agent`
          : undefined
      }
      onChange={(e) => setModel(e.target.value)}
      className="max-w-[260px] rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-[0.85rem] text-foreground disabled:opacity-60"
    >
      {options.length === 0 ? (
        <option value="" disabled>
          Waiting for gateway...
        </option>
      ) : (
        options.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))
      )}
    </select>
  );
}
