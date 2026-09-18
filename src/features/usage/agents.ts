import type { AgentUsage } from "../../types/usage";

/**
 * Single source of truth for agent presentation metadata: canonical order,
 * display names, the CSS colour tokens they pin, and the sigil asset key for
 * the inline SVG mark (marks doc §2). Unknown agent ids are never lost or
 * hard-coded per-UI-site; they collapse to the neutral `--agent-unknown`
 * colours and the neutral diamond sigil, and sort after the five known
 * agents. The colour *values* live only in `App.css` theme tokens —
 * components reference them through the `--` variable names below.
 */
export interface AgentMeta {
  id: string;
  /** Display name used when the data has none of its own to offer. */
  displayName: string;
  /** Sigil asset key under `src/assets/agent-marks/` (without extension),
   * rendered as an inline stroke-currentColor SVG at 15px. Known agents pin
   * their redrawn official mark; ids without a drawn mark (and unknown ids)
   * fall back to the neutral diamond. */
  sigil: string;
  /** Readable CSS custom-property name, e.g. "--agent-claude". */
  colorVar: string;
  /** Readable CSS custom-property name for the 20% chip-selection tint. */
  softVar: string;
  /** Ordering position; known agents are 1..5, unknown agents sort last. */
  sort: number;
}

const KNOWN_AGENTS: Record<string, AgentMeta> = {
  claude: {
    id: "claude",
    displayName: "Claude Code",
    sigil: "claude-code",
    colorVar: "--agent-claude",
    softVar: "--agent-claude-soft",
    sort: 1,
  },
  // Claude Desktop sits with Claude Code rather than at the end of the list:
  // the two are the same product family, and reading them together is how a
  // user tells which surface spent the tokens. No official product mark has
  // been redrawn for it yet, so it carries the neutral diamond in its own
  // identity colour — shape stays neutral, colour still disambiguates.
  "claude-desktop": {
    id: "claude-desktop",
    displayName: "Claude Desktop",
    sigil: "unknown",
    colorVar: "--agent-claude-desktop",
    softVar: "--agent-claude-desktop-soft",
    sort: 2,
  },
  codex: {
    id: "codex",
    displayName: "Codex",
    sigil: "codex",
    colorVar: "--agent-codex",
    softVar: "--agent-codex-soft",
    sort: 3,
  },
  antigravity: {
    id: "antigravity",
    displayName: "Antigravity",
    sigil: "antigravity",
    colorVar: "--agent-antigravity",
    softVar: "--agent-antigravity-soft",
    sort: 4,
  },
  opencode: {
    id: "opencode",
    displayName: "OpenCode",
    sigil: "opencode",
    colorVar: "--agent-opencode",
    softVar: "--agent-opencode-soft",
    sort: 5,
  },
};

/** Canonical fixed order for the known agents: Claude Code, Claude Desktop,
 * Codex, Antigravity, OpenCode (bottom-up in the stacked chart too). The two
 * Claude surfaces lead and stay adjacent, so the inline filter capacity can
 * never push Claude Desktop behind the overflow disclosure while Claude Code
 * is shown. */
export const KNOWN_AGENT_IDS = [
  "claude",
  "claude-desktop",
  "codex",
  "antigravity",
  "opencode",
];

const UNKNOWN_SORT = Number.MAX_SAFE_INTEGER;

/** Metadata for an agent id. Unknown ids keep their real css-colour fallback
 * (rendered from the data) but always resolve to `--agent-unknown` colours,
 * the neutral diamond sigil, and sort last with a stable, deterministic
 * secondary order. */
export function agentMeta(id: string): AgentMeta {
  return (
    KNOWN_AGENTS[id] ?? {
      id,
      displayName: id,
      sigil: "unknown",
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