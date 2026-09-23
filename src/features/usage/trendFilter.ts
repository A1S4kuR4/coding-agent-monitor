import type { DailyUsage } from "../../types/usage";

/**
 * Trend-filter view-model (T04). The filter affects the seven-day trend only —
 * today's summary is never filtered — and both rules here are pure so the
 * fallback and capacity behaviour is unit-testable without a DOM.
 */

/** How many agent chips render inline before the rest move into the "More
 * agents" disclosure. Four distinct colours stay comfortably readable; 17
 * never spill into a wall of chips. */
export const MAX_VISIBLE_AGENT_CHIPS = 4;

/** Resolve the effective trend filter. A selected agent that has vanished from
 * the visible range falls back deterministically to All (null) instead of
 * rendering an empty chart — the filter only ever selects agents the trend
 * actually contains. */
export function effectiveAgentFilter(
  filter: string | null,
  days: DailyUsage[],
): string | null {
  if (filter === null) return null;
  return days.some((day) => day.agents.some((a) => a.id === filter))
    ? filter
    : null;
}

export interface FilterAgent {
  id: string;
  displayName: string;
}

export interface FilterChipLayout {
  /** Chips rendered inline (always includes the selected agent). */
  visible: FilterAgent[];
  /** Chips inside the "More agents" disclosure. */
  hidden: FilterAgent[];
}

/** Split the recognized agents into inline and overflow chips. The rule is
 * deterministic: the first `capacity` agents in the already canonically-sorted
 * list stay inline; a selected agent beyond the capacity swaps in for the last
 * inline slot so the active selection is always visible, and the swapped-out
 * agent moves into the disclosure. The split never reorders agents. */
export function visibleFilterAgents(
  agents: FilterAgent[],
  selected: string | null,
  capacity: number = MAX_VISIBLE_AGENT_CHIPS,
): FilterChipLayout {
  if (agents.length <= capacity) {
    return { visible: agents, hidden: [] };
  }
  const selectedAgent =
    selected !== null ? agents.find((a) => a.id === selected) ?? null : null;
  if (selectedAgent === null) {
    return { visible: agents.slice(0, capacity), hidden: agents.slice(capacity) };
  }
  const inline = agents.slice(0, capacity);
  if (inline.some((a) => a.id === selectedAgent.id)) {
    return { visible: inline, hidden: agents.slice(capacity) };
  }
  return {
    visible: [...agents.slice(0, capacity - 1), selectedAgent],
    hidden: [
      ...agents.slice(capacity - 1).filter((a) => a.id !== selectedAgent.id),
    ],
  };
}
