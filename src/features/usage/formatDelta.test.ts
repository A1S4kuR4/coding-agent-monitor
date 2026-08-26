import { describe, expect, it } from "vitest";
import { formatDelta } from "./formatDelta";

describe("formatDelta", () => {
  it("formats a normal rise above 1%", () => {
    expect(formatDelta(93_890_000, 56_491_131)).toMatchObject({
      kind: "up",
      label: "▲ +66.2% vs 昨日",
      percent: "+66.2%",
    });
  });

  it("formats a normal fall below -1%", () => {
    expect(formatDelta(18_000_000, 20_000_000)).toMatchObject({
      kind: "down",
      label: "▼ -10.0% vs 昨日",
    });
    expect(formatDelta(500, 1000)).toMatchObject({
      kind: "down",
      label: "▼ -50.0% vs 昨日",
    });
  });

  it("handles yesterday === 0 with today > 0", () => {
    expect(formatDelta(20_000_000, 0)).toMatchObject({
      kind: "no-usage-yesterday",
      label: "— 昨日无使用",
    });
  });

  it("hides a double-zero day", () => {
    expect(formatDelta(0, 0)).toEqual({ kind: "none", label: null, percent: null });
  });

  it("formats today === 0 with yesterday > 0 as a full drop", () => {
    expect(formatDelta(0, 12_000_000)).toMatchObject({
      kind: "down",
      label: "▼ -100.0% vs 昨日",
      percent: "-100.0%",
    });
  });

  it("marks an absolute change under 1% as flat, keeping its sign", () => {
    expect(formatDelta(100_060, 100_000)).toMatchObject({
      kind: "flat",
      label: "• +0.1% vs 昨日",
      percent: "+0.1%",
    });
    expect(formatDelta(99_940, 100_000)).toMatchObject({
      kind: "flat",
      label: "• -0.1% vs 昨日",
      percent: "-0.1%",
    });
  });

  it("treats exactly +/-1% as a real move, not flat", () => {
    expect(formatDelta(101_000, 100_000)).toMatchObject({ kind: "up" });
    expect(formatDelta(99_000, 100_000)).toMatchObject({ kind: "down" });
  });

  it("ranks the 1% boundary strictly", () => {
    // 0.99% stays flat, 1.00% is up.
    expect(formatDelta(100_990, 100_000).kind).toBe("flat");
    expect(formatDelta(101_000, 100_000).kind).toBe("up");
  });

  it("returns no-yesterday when there is no prior data point", () => {
    expect(formatDelta(5_000_000, undefined)).toMatchObject({
      kind: "no-yesterday",
      label: "— 无前一日数据",
    });
  });

  it("never returns NaN/Infinity for missing or non-finite inputs", () => {
    expect(formatDelta(NaN, 100)).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(Infinity, 100)).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(null, 100)).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(100, NaN)).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(100, null)).toEqual({ kind: "none", label: null, percent: null });
    expect(formatDelta(undefined, 100)).toEqual({ kind: "none", label: null, percent: null });
  });
});