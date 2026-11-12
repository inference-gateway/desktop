/** The CLI's agent modes, in the order its Shift+Tab cycles them. */
export const AGENT_MODES = ["standard", "plan", "auto", "auto-with-judge"] as const;
export type AgentMode = (typeof AGENT_MODES)[number];

/** How the user decides a plan: the CLI's plan approval actions. */
export type PlanAction = "accept" | "accept_standard" | "reject";

/** What each mode does, as the mode button's accessible name. */
export const MODE_DESCRIPTIONS: Record<AgentMode, string> = {
  standard: "Standard mode - protected tool actions ask first",
  plan: "Plan mode - the agent plans before it changes anything",
  auto: "Auto mode - tool actions run without asking",
  "auto-with-judge": "Auto with judge mode - a judge model decides protected tool actions",
};

/** The short label the mode button shows next to its icon. */
export const MODE_LABELS: Record<AgentMode, string> = {
  standard: "Standard",
  plan: "Plan",
  auto: "Auto",
  "auto-with-judge": "Auto+Judge",
};

/** The mode after `mode`, wrapping around like the CLI. */
export function nextMode(mode: AgentMode): AgentMode {
  return AGENT_MODES[(AGENT_MODES.indexOf(mode) + 1) % AGENT_MODES.length];
}

/** A stored mode, or standard when it is missing or unknown. */
export function parseMode(value: string | null): AgentMode {
  return AGENT_MODES.find((mode) => mode === value) ?? "standard";
}

/** The mode a decided plan runs in, as the CLI switches it, or null when the
 * plan is sent back and the chat keeps planning. */
export function modeAfterPlan(action: PlanAction): AgentMode | null {
  if (action === "accept") return "auto";
  if (action === "accept_standard") return "standard";
  return null;
}
