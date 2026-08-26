import type { AgentUsage } from "../../types/usage";

/**
 * Active (non-zero) agent rows for a day's list. The Rust adapter already
 * excludes zero-token agents and sorts by tokens descending, but the view
 * re-applies the invariant so it can never render a zero row from a stale or
 * foreign contract.
 */
export function activeAgentRows(agents: AgentUsage[]): AgentUsage[] {
  return agents.filter((agent) => agent.tokens > 0);
}