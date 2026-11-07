import { useDesktop } from "@/store";
import { agentThreadModel } from "@/lib/agents";

// Native <select> (AXPopUpButton) - required by the e2e harness; do not replace
// with a custom dropdown.
export function ModelSelect() {
  const { models, model, setModel, enabled, agents, currentAgent, setAgentModel, setError } = useDesktop();
  const agent = agents.find((a) => a.name === currentAgent);
  const pick = (picked: string) => {
    if (agent) setAgentModel(agent, picked).catch((e) => setError(String(e)));
    else setModel(picked);
  };
  return (
    <select
      id="model-select"
      value={agentThreadModel(agent, model)}
      disabled={!enabled}
      onChange={(e) => pick(e.target.value)}
      className="max-w-[260px] rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-[0.85rem] text-foreground disabled:opacity-60"
    >
      {models.length === 0 ? (
        <option value="" disabled>
          Waiting for gateway...
        </option>
      ) : (
        models.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))
      )}
    </select>
  );
}
