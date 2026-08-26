import type { AgentUsage } from "../../types/usage";

/**
 * Single source of truth for agent presentation metadata: canonical order,
 * display names, the CSS colour tokens they pin, and their 20% soft-token for
 * the selected-chip background. Unknown agent ids are never lost or hard-coded
 * per-UI-site; they collapse to the neutral `--agent-unknown` colours and sort
 * after the four known agents. The colour *values* live only in `App.css` theme
 * tokens — components reference them through the `--` variable names below.
 */
export interface AgentMeta {
  id: string;
  /** Display name used when the data has none of its own to offer. */
  displayName: string;
  /** Readable CSS custom-property name, e.g. "--agent-claude". */
  colorVar: string;
  /** Readable CSS custom-property name for the 20% chip-selection tint. */
  softVar: string;
  /** Ordering position; known agents are 1..4, unknown agents sort last. */
  sort: number;
}

const KNOWN_AGENTS: Record<string, AgentMeta> = {
  claude: {
    id: "claude",
    displayName: "Claude Code",
    colorVar: "--agent-claude",
    softVar: "--agent-claude-soft",
    sort: 1,
  },
  codex: {
    id: "codex",
    displayName: "Codex",
    colorVar: "--agent-codex",
    softVar: "--agent-codex-soft",
    sort: 2,
  },
  antigravity: {
    id: "antigravity",
    displayName: "Antigravity",
    colorVar: "--agent-antigravity",
    softVar: "--agent-antigravity-soft",
    sort: 3,
  },
  opencode: {
    id: "opencode",
    displayName: "OpenCode",
    colorVar: "--agent-opencode",
    softVar: "--agent-opencode-soft",
    sort: 4,
  },
};

/** Canonical fixed order for the known agents: Claude Code, Codex, Antigravity,
 * OpenCode (bottom-up in the stacked chart too). */
export const KNOWN_AGENT_IDS = ["claude", "codex", "antigravity", "opencode"];

const UNKNOWN_SORT = Number.MAX_SAFE_INTEGER;

/** Metadata for an agent id. Unknown ids keep their real css-colour fallback
 * (rendered from the data) but always resolve to `--agent-unknown` colours and
 * sort last with a stable, deterministic secondary order. */
export function agentMeta(id: string): AgentMeta {
  return (
    KNOWN_AGENTS[id] ?? {
      id,
      displayName: id,
      colorVar: "--agent-unknown",
      softVar: "--agent-unknown-soft",
      sort: UNKNOWN_SORT,
    }
  );
}

/** Comparator shared by both the sorted chart segments and the recognized-chip
 * list, so ordering never diverges between the two surfaces. Known agents line
 * up by their canonical position; unknown agents use display name for a stable
 * ordering that still renders after every known agent. */
export function compareByMeta(
  a: { id: string; displayName: string },
  b: { id: string; displayName: string },
): number {
  const am = agentMeta(a.id);
  const bm = agentMeta(b.id);
  if (am.sort !== bm.sort) return am.sort - bm.sort;
  return a.displayName.localeCompare(b.displayName);
}

/** Sort a list of agents (e.g. one day's contribution) into the fixed
 * presentation order. The input is not mutated. */
export function sortAgents(agents: AgentUsage[]): AgentUsage[] {
  return [...agents].sort(compareByMeta);
}