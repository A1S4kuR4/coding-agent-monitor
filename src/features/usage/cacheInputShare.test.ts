import { describe, expect, it } from "vitest";

import { cacheInputShare } from "./cacheInputShare";

describe("cacheInputShare", () => {
  it("computes the model cached-input share over input+cacheRead+cacheCreation", () => {
    // 50 / (100 + 50 + 0) = 1/3.
    expect(cacheInputShare(100, 50, 0)).toBeCloseTo(1 / 3, 10);
  });

  it("includes cache creation in the denominator, not just input+cacheRead", () => {
    // 100 / (100 + 100 + 100) = 1/3 — if cache creation were excluded the share
    // would be 100/200 = 0.5.
    expect(cacheInputShare(100, 100, 100)).toBeCloseTo(1 / 3, 10);
  });

  it("returns null for a zero denominator so callers never render a misleading 0%", () => {
    // input + cacheRead + cacheCreation === 0 → no data, hide the share.
    expect(cacheInputShare(0, 0, 0)).toBeNull();
  });

  it("returns null for non-finite inputs", () => {
    expect(cacheInputShare(Number.POSITIVE_INFINITY, 1, 1)).toBeNull();
  });
});