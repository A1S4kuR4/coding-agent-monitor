import type { AgentUsage } from "../../types/usage";

/**
 * Single source of truth for agent presentation metadata: canonical order,
 * display names, the CSS colour tokens they pin, and their 20% soft-token for
 * the selected-chip background. Unknown agent ids are never lost or hard-coded
 * per-UI-site; they collapse to the neutral `--agent-unknown` colours and sort
 * after the five known agents. The colour *values* live only in `App.css` theme
 * tokens — components reference them through the `--` variable names below.
 */
export interface AgentMeta {
  id: string;
  /** Display name used when the data has none of its own to offer. */
  displayName: string;
  /** Identity mark, pinned per known agent. Two known agents must never share
   * one: the derived form takes the first two characters of the name, which
   * collides for products that share a first word (Claude Code / Claude
   * Desktop). Unknown agents keep deriving theirs from the display name. */
  mark: string;
  /** Readable CSS custom-property name, e.g. "--agent-claude". */
  colorVar: string;
  /** Readable CSS custom-property name for the 20% chip-selection tint. */
  softVar: string;
  /** Ordering position; known agents are 1..5, unknown agents sort last. */
  sort: number;
}

const KNOWN_AGENTS: Record<string, AgentMeta> = {
  claude: {
    mark: "CL",
    id: "claude",
    displayName: "Claude Code",
    colorVar: "--agent-claude",
    softVar: "--agent-claude-soft",
    sort: 1,
  },
  // Claude Desktop sits with Claude Code rather than at the end of the list:
  // the two are the same product family, and reading them together is how a
  // user tells which surface spent the tokens.
  "claude-desktop": {
    mark: "CD",
    id: "claude-desktop",
    displayName: "Claude Desktop",
    colorVar: "--agent-claude-desktop",
    softVar: "--agent-claude-desktop-soft",
    sort: 2,
  },
  codex: {
    mark: "CO",
    id: "codex",
    displayName: "Codex",
    colorVar: "--agent-codex",
    softVar: "--agent-codex-soft",
    sort: 3,
  },
  antigravity: {
    mark: "AN",
    id: "antigravity",
    displayName: "Antigravity",
    colorVar: "--agent-antigravity",
    softVar: "--agent-antigravity-soft",
    sort: 4,
  },
  opencode: {
    mark: "OP",
    id: "opencode",
    displayName: "OpenCode",
    colorVar: "--agent-opencode",
    softVar: "--agent-opencode-soft",
    sort: 5,
  },
};

/** Deterministic 1–2 character identity mark rendered next to an agent's name
 * (list rows, filter chips, tooltip legends, day details). Colour alone cannot
 * identify agents — especially the unknown ones, which share one neutral token
 * — so every surface shows this same monogram derived from the display name:
 * the first two alphanumeric characters, uppercased. Dynamic user data is only
 * transformed, never dictionary-matched. */
export function agentMark(displayName: string): string {
  const chars = [...displayName].filter((c) => /\p{L}|\p{N}/u.test(c));
  return (chars[0] ?? "").toUpperCase() + (chars[1] ?? "").toUpperCase();
}

/** The mark to render for an agent: its pinned mark when it is a known agent,
 * otherwise the one derived from the name the data supplied. Every surface uses
 * this rather than `agentMark` directly, so a known agent's mark is decided in
 * exactly one place. */
export function agentMarkFor(id: string, displayName: string): string {
  return KNOWN_AGENTS[id]?.mark ?? agentMark(displayName);
}

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
 * (rendered from the data) but always resolve to `--agent-unknown` colours and
 * sort last with a stable, deterministic secondary order. */
export function agentMeta(id: string): AgentMeta {
  return (
    KNOWN_AGENTS[id] ?? {
      id,
      displayName: id,
      mark: agentMark(id),
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