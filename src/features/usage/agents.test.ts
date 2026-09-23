import { describe, expect, it } from "vitest";
import {
  agentMeta,
  compareByMeta,
  sortAgents,
  KNOWN_AGENT_IDS,
} from "./agents";
import type { AgentUsage } from "../../types/usage";

const dagent = (id: string, displayName: string, tokens = 1): AgentUsage => ({
  id,
  displayName,
  tokens,
  reasoningTokens: 0,
  unclassifiedTokens: 0,
  models: [],
});

describe("agentMeta", () => {
  it("maps the five known agent ids to their colour tokens, names and sort", () => {
    expect(agentMeta("claude")).toMatchObject({
      displayName: "Claude Code",
      colorVar: "--agent-claude",
      softVar: "--agent-claude-soft",
      sort: 1,
    });
    expect(agentMeta("claude-desktop")).toMatchObject({
      displayName: "Claude Desktop",
      colorVar: "--agent-claude-desktop",
      softVar: "--agent-claude-desktop-soft",
      sort: 2,
    });
    expect(agentMeta("codex")).toMatchObject({
      displayName: "Codex",
      colorVar: "--agent-codex",
      sort: 3,
    });
    expect(agentMeta("antigravity")).toMatchObject({
      displayName: "Antigravity",
      colorVar: "--agent-antigravity",
      sort: 4,
    });
    expect(agentMeta("opencode")).toMatchObject({
      displayName: "OpenCode",
      colorVar: "--agent-opencode",
      sort: 5,
    });
  });

  it("falls back unknown ids to the neutral token and a last sort position", () => {
    expect(agentMeta("future-agent-xyz")).toMatchObject({
      colorVar: "--agent-unknown",
      softVar: "--agent-unknown-soft",
      sort: Number.MAX_SAFE_INTEGER,
    });
  });
});

describe("sigils (marks doc §2)", () => {
  it("pins a sigil asset to every known agent", () => {
    expect(agentMeta("claude").sigil).toBe("claude-code");
    expect(agentMeta("codex").sigil).toBe("codex");
    expect(agentMeta("antigravity").sigil).toBe("antigravity");
    expect(agentMeta("opencode").sigil).toBe("opencode");
  });

  it("falls back to the neutral diamond for unknown agents", () => {
    expect(agentMeta("future-agent-xyz").sigil).toBe("unknown");
  });

  it("Claude Desktop has no drawn mark yet and carries the neutral diamond", () => {
    // Marks doc §2 covers four agents; Claude Desktop (added with its own
    // colour) has no redrawn official mark, so it keeps the neutral diamond
    // in its own identity colour until one is drawn.
    expect(agentMeta("claude-desktop").sigil).toBe("unknown");
  });

  it("the retired letter monograms no longer exist", () => {
    // The ring+monogram AgentMark mode is retired; only the sigil asset key
    // remains on the metadata.
    const meta = agentMeta("claude") as unknown as Record<string, unknown>;
    expect(meta.mark).toBeUndefined();
  });
});

describe("KNOWN_AGENT_IDS", () => {
  it("is in the canonical fixed order", () => {
    expect(KNOWN_AGENT_IDS).toEqual([
      "claude",
      "claude-desktop",
      "codex",
      "antigravity",
      "opencode",
    ]);
  });
});

describe("compareByMeta / sortAgents", () => {
  it("orders known agents canonically and puts unknown agents after them", () => {
    const input = [
      dagent("opencode", "OpenCode"),
      dagent("claude", "Claude Code"),
      dagent("mystery", "Mystery Agent"),
      dagent("antigravity", "Antigravity"),
      dagent("codex", "Codex"),
      dagent("claude-desktop", "Claude Desktop"),
    ];
    const sorted = sortAgents(input);
    expect(sorted.map((a) => a.id)).toEqual([
      "claude",
      "claude-desktop",
      "codex",
      "antigravity",
      "opencode",
      "mystery",
    ]);
  });

  it("does not mutate the input array", () => {
    const input = [dagent("codex", "Codex"), dagent("claude", "Claude Code")];
    sortAgents(input);
    expect(input.map((a) => a.id)).toEqual(["codex", "claude"]);
  });

  it("orders unknown agents deterministically by display name", () => {
    const sorted = sortAgents([
      dagent("z", "Zed Agent"),
      dagent("a", "Alpha Agent"),
      dagent("b", "Beta Agent"),
    ]);
    expect(sorted.map((a) => a.displayName)).toEqual([
      "Alpha Agent",
      "Beta Agent",
      "Zed Agent",
    ]);
  });

  it("compareByMeta agrees with sortAgents for chip-shaped items", () => {
    const items = [
      { id: "mystery", displayName: "Mystery" },
      { id: "claude", displayName: "Claude Code" },
    ];
    expect(items.sort(compareByMeta).map((i) => i.id)).toEqual(["claude", "mystery"]);
  });
});
