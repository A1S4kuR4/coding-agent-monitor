import type {
  AgentUsage,
  DailyUsage,
  ModelUsage,
  TokenBreakdown,
  UsageSummary,
} from "../types/usage";

/**
 * Fixed, deterministic presentation fixture used ONLY by the browser test
 * harness and screenshot scripts. It is never served through the production
 * data path: `fetchUsageSummary` still invokes the Rust command unless the
 * harness has installed the Tauri IPC mock.
 *
 * Values are chosen so the existing formatting rules produce the documented
 * display strings, e.g. 56_491_131 → "56.49M", 93_890_000 → "93.89M".
 *
 * Days (today = 08/25):
 *   08/19  0            — zero day (spec §13.1)
 *   08/20  20,000,000   — "yesterday 0 / today > 0" boundary after 08/19
 *   08/21  18,000,000   — a day whose total is DOWN vs 08/20 (chart delta ▼)
 *   08/22  18,050,000   — absolute change < 1% vs 08/21 (flat)
 *   08/23  40,000,000   — includes an agent (opencode) with value 0
 *   08/24  56,491,131   — 56.49M, contains Claude Code + Codex + Antigravity
 *   08/25  93,890,000   — 93.89M today; prev 56.49M → header delta +66.2%
 */
function breakdownFor(total: number): TokenBreakdown {
  const inputTokens = Math.floor(total * 0.3);
  const outputTokens = Math.floor(total * 0.1);
  const cacheReadTokens = Math.floor(total * 0.6);
  const otherTokens =
    total - inputTokens - outputTokens - cacheReadTokens - 0;
  return {
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheCreationTokens: 0,
    otherTokens,
  };
}

function model(
  modelName: string,
  total: number,
  inputRatio = 0.3,
  outputRatio = 0.1,
): ModelUsage {
  const inputTokens = Math.floor(total * inputRatio);
  const outputTokens = Math.floor(total * outputRatio);
  const cacheReadTokens = total - inputTokens - outputTokens;
  return {
    modelName,
    modelDisplayName: modelName,
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheCreationTokens: 0,
    totalTokens: total,
  };
}

function day(
  date: string,
  agents: AgentUsage[],
): DailyUsage {
  const totalTokens = agents.reduce((sum, a) => sum + a.tokens, 0);
  return {
    date,
    totalTokens,
    tokenBreakdown: breakdownFor(totalTokens),
    estimatedCostUsd: totalTokens > 0 ? +totalTokens * 4e-6 : null,
    cacheReadShare: totalTokens > 0 ? 0.6 : null,
    agents,
  };
}

const agent = (
  id: string,
  displayName: string,
  tokens: number,
  models: ModelUsage[],
): AgentUsage => ({ id, displayName, tokens, models });

export const e2eFixture: UsageSummary = {
  collectedAt: "2026-08-25T07:00:00.000Z",
  today: day("2026-08-25", [
    agent("claude", "Claude Code", 33_973_315, [
      model("deepseek-v4-flash", 30_000_000),
    ]),
    agent("codex", "Codex", 33_280_719, [
      model("gpt-5.6-sol", 32_000_000),
    ]),
    agent("opencode", "OpenCode", 18_165_661, []),
    agent("antigravity", "Antigravity", 8_470_305, [
      model("gemini-3.7-flash", 8_000_000),
    ]),
  ]),
  last7Days: [
    day("2026-08-19", []),
    day("2026-08-20", [
      agent("claude", "Claude Code", 8_000_000, [model("deepseek-v4-flash", 8_000_000)]),
      agent("codex", "Codex", 12_000_000, [model("gpt-5.6-sol", 12_000_000)]),
    ]),
    day("2026-08-21", [
      agent("claude", "Claude Code", 9_000_000, [model("deepseek-v4-flash", 9_000_000)]),
      agent("codex", "Codex", 9_000_000, [model("gpt-5.6-sol", 9_000_000)]),
    ]),
    day("2026-08-22", [
      agent("claude", "Claude Code", 10_050_000, [model("deepseek-v4-flash", 10_050_000)]),
      agent("codex", "Codex", 8_000_000, [model("gpt-5.6-sol", 8_000_000)]),
    ]),
    day("2026-08-23", [
      agent("claude", "Claude Code", 24_000_000, [model("deepseek-v4-flash", 24_000_000)]),
      agent("codex", "Codex", 16_000_000, [model("gpt-5.6-sol", 16_000_000)]),
    ]),
    day("2026-08-24", [
      agent("claude", "Claude Code", 29_299_928, [model("deepseek-v4-flash", 29_299_928)]),
      agent("codex", "Codex", 26_176_447, [model("gpt-5.6-sol", 26_176_447)]),
      agent("antigravity", "Antigravity", 1_014_756, [model("gemini-3.7-flash", 1_014_756)]),
    ]),
    // Today mirrors summary.today.date so the trend window attributes agents.
    day("2026-08-25", [
      agent("claude", "Claude Code", 33_973_315, [model("deepseek-v4-flash", 30_000_000)]),
      agent("codex", "Codex", 33_280_719, [model("gpt-5.6-sol", 32_000_000)]),
      agent("opencode", "OpenCode", 18_165_661, []),
      agent("antigravity", "Antigravity", 8_470_305, [model("gemini-3.7-flash", 8_000_000)]),
    ]),
  ],
};