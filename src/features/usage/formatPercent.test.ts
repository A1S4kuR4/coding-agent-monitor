import { describe, expect, it } from "vitest";

import { formatPercent } from "./formatPercent";

describe("formatPercent", () => {
  it("formats a normal ratio as a rounded percentage", () => {
    expect(formatPercent(0)).toBe("0%");
    expect(formatPercent(0.708133)).toBe("71%");
    expect(formatPercent(1)).toBe("100%");
  });

  it("rounds to the nearest integer percent at the boundaries", () => {
    expect(formatPercent(0.995)).toBe("100%");
    expect(formatPercent(0.004)).toBe("0%");
  });

  it("returns null for an unavailable (zero-denominator) share", () => {
    expect(formatPercent(null)).toBeNull();
    expect(formatPercent(Number.NaN)).toBeNull();
    expect(formatPercent(Number.POSITIVE_INFINITY)).toBeNull();
  });
});