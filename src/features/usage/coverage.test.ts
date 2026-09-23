import { describe, expect, it } from "vitest";

import type { CoverageInfo, SourceDiagnostic } from "../../types/usage";
import { coverageText } from "./coverage";

function diagnostic(overrides: Partial<SourceDiagnostic>): SourceDiagnostic {
  return {
    kind: "corruptRecord",
    agentId: "claude",
    agentDisplayName: "Claude Code",
    count: 1,
    ...overrides,
  };
}

describe("coverageText", () => {
  it("returns empty text for complete coverage", () => {
    const coverage: CoverageInfo = { status: "complete", diagnostics: [] };
    expect(coverageText(coverage, "en")).toBe("");
  });

  it("never invents text for a possiblyIncomplete status without diagnostics", () => {
    const coverage: CoverageInfo = { status: "possiblyIncomplete", diagnostics: [] };
    expect(coverageText(coverage, "en")).toBe("");
  });

  it("singularizes a single skipped record", () => {
    const coverage: CoverageInfo = {
      status: "possiblyIncomplete",
      diagnostics: [diagnostic({})],
    };
    expect(coverageText(coverage, "en")).toBe(
      "Coverage may be incomplete — Claude Code: 1 record was malformed and skipped.",
    );
  });

  it("pluralizes and joins multiple agents and kinds", () => {
    const coverage: CoverageInfo = {
      status: "possiblyIncomplete",
      diagnostics: [
        diagnostic({ count: 3 }),
        diagnostic({
          kind: "databaseError",
          agentId: "codex",
          agentDisplayName: "Codex",
        }),
      ],
    };
    expect(coverageText(coverage, "en")).toBe(
      "Coverage may be incomplete — Claude Code: 3 records were malformed and skipped; Codex: 1 database read failed.",
    );
  });

  it("renders the same sanitized facts in Chinese, keeping agent names untranslated", () => {
    const coverage: CoverageInfo = {
      status: "possiblyIncomplete",
      diagnostics: [
        diagnostic({ count: 2 }),
        diagnostic({
          kind: "databaseError",
          agentId: "codex",
          agentDisplayName: "Codex",
        }),
      ],
    };
    expect(coverageText(coverage, "zh-CN")).toBe(
      "覆盖可能不完整 — Claude Code: 2 条记录格式错误已跳过; Codex: 1 次数据库读取失败。",
    );
  });
});
