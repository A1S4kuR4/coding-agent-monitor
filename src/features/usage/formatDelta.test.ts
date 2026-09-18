import { describe, expect, it } from "vitest";
import { formatDelta } from "./formatDelta";

describe("formatDelta", () => {
  it("formats a normal rise above 1% with a neutral full-day basis", () => {
    expect(formatDelta(93_890_000, 56_491_131, "en", "yesterday-full-day")).toMatchObject({
      kind: "up",
      label: "▲ +66.2% vs yesterday (full day)",
      percent: "+66.2%",
    });
    expect(formatDelta(93_890_000, 56_491_131, "zh-CN", "yesterday-full-day")).toMatchObject({
      label: "▲ +66.2% 较昨日全天",
    });
  });

  it("formats a normal fall below -1%", () => {
    expect(formatDelta(18_000_000, 20_000_000, "en")).toMatchObject({
      kind: "down",
      label: "▼ -10.0% vs previous day",
    });
    expect(formatDelta(500, 1000, "en")).toMatchObject({
      kind: "down",
      label: "▼ -50.0% vs previous day",
    });
  });

  it("defaults to the previous-day basis for full-day pairs", () => {
    expect(formatDelta(18_000_000, 20_000_000, "zh-CN")).toMatchObject({
      label: "▼ -10.0% 较前一日",
    });
  });

  it("handles yesterday === 0 with today > 0", () => {
    expect(formatDelta(20_000_000, 0, "en")).toMatchObject({
      kind: "no-usage-yesterday",
      label: "— yesterday had no usage",
    });
    expect(formatDelta(20_000_000, 0, "zh-CN")).toMatchObject({
      label: "— 昨日无用量",
    });
  });

  it("hides a double-zero day", () => {
    expect(formatDelta(0, 0, "en")).toEqual({ kind: "none", label: null, percent: null });
  });

  it("formats today === 0 with yesterday > 0 as a full drop", () => {
    expect(formatDelta(0, 12_000_000, "en")).toMatchObject({
      kind: "down",
      label: "▼ -100.0% vs previous day",
      percent: "-100.0%",
    });
  });

  it("marks an absolute change under 1% as flat, keeping its sign", () => {
    expect(formatDelta(100_060, 100_000, "en")).toMatchObject({
      kind: "flat",
      label: "• +0.1% vs previous day",
      percent: "+0.1%",
    });
    expect(formatDelta(99_940, 100_000, "en")).toMatchObject({
      kind: "flat",
      label: "• -0.1% vs previous day",
      percent: "-0.1%",
    });
  });

  it("treats exactly +/-1% as a real move, not flat", () => {
    expect(formatDelta(101_000, 100_000, "en").kind).toBe("up");
    expect(formatDelta(99_000, 100_000, "en").kind).toBe("down");
  });

  it("ranks the 1% boundary strictly", () => {
    // 0.99% stays flat, 1.00% is up.
    expect(formatDelta(100_990, 100_000, "en").kind).toBe("flat");
    expect(formatDelta(101_000, 100_000, "en").kind).toBe("up");
  });

  it("returns no-yesterday when there is no prior data point", () => {
    expect(formatDelta(5_000_000, undefined, "en")).toMatchObject({
      kind: "no-yesterday",
      label: "— no prior-day data",
    });
    expect(formatDelta(5_000_000, undefined, "zh-CN")).toMatchObject({
      label: "— 无前一日数据",
    });
  });

  it("never returns NaN/Infinity for missing or non-finite inputs", () => {
    expect(formatDelta(NaN, 100, "en")).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(Infinity, 100, "en")).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(null, 100, "en")).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(100, NaN, "en")).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(100, null, "en")).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(undefined, 100, "en")).toEqual({ kind: "none", label: null, percent: null });
  });

  it("compares week buckets against the previous week, stated in the label", () => {
    expect(formatDelta(30_000_000, 20_000_000, "en", "previous-week")).toMatchObject({
      kind: "up",
      label: "▲ +50.0% vs previous week",
    });
    expect(formatDelta(10_000_000, 20_000_000, "zh-CN", "previous-week")).toMatchObject({
      label: "▼ -50.0% 较前一周",
    });
  });

  it("renders no delta line for week buckets without a prior bucket", () => {
    // The first bucket has nothing before the window, and the partial current
    // bucket is passed with no prior on purpose — a partial-vs-full-week
    // comparison would read as a collapse.
    expect(formatDelta(5_000_000, undefined, "en", "previous-week")).toEqual({
      kind: "none",
      label: null,
      percent: null,
    });
    expect(formatDelta(5_000_000, undefined, "zh-CN", "previous-week")).toEqual({
      kind: "none",
      label: null,
      percent: null,
    });
  });
});
