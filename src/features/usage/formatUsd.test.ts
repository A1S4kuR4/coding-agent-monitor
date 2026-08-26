import { describe, expect, it } from "vitest";

import { formatUsd } from "./formatUsd";

describe("formatUsd", () => {
  it("formats a normal value with two decimals", () => {
    expect(formatUsd(12)).toBe("$12.00");
    expect(formatUsd(12.345)).toBe("$12.35");
    expect(formatUsd(0.36)).toBe("$0.36");
  });

  it("renders a genuinely zero cost as zero", () => {
    expect(formatUsd(0)).toBe("$0.00");
  });

  it("rounds tiny values without inventing precision", () => {
    expect(formatUsd(0.004)).toBe("$0.00");
    expect(formatUsd(0.006)).toBe("$0.01");
  });

  it("returns null for a missing cost so it is never shown as $0.00", () => {
    expect(formatUsd(null)).toBeNull();
    expect(formatUsd(Number.NaN)).toBeNull();
    expect(formatUsd(Number.POSITIVE_INFINITY)).toBeNull();
  });
});