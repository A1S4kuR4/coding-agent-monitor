import { describe, expect, it } from "vitest";

import type { AgentUsage } from "../../types/usage";
import { activeAgentRows } from "./usageRows";

const row = (id: string, tokens: number): AgentUsage => ({
  id,
  displayName: id,
  tokens,
  models: [],
});

describe("activeAgentRows", () => {
  it("drops zero-token agents so no empty row is ever rendered", () => {
    expect(activeAgentRows([row("claude", 0), row("codex", 5)])).toEqual([
      row("codex", 5),
    ]);
  });

  it("preserves the server-supplied order for active agents", () => {
    const rows = [row("codex", 5_000_000), row("claude", 8_000_000)];
    expect(activeAgentRows(rows)).toEqual(rows);
  });

  it("renders unknown agents as-is rather than hiding them", () => {
    const future = row("future-agent", 100);
    expect(activeAgentRows([future])).toEqual([future]);
  });

  it("returns an empty list when nothing is active", () => {
    expect(activeAgentRows([])).toEqual([]);
    expect(activeAgentRows([row("claude", 0)])).toEqual([]);
  });
});