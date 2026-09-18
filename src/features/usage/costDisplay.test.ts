import { describe, expect, it } from "vitest";

import { costDisplay } from "./costDisplay";

describe("costDisplay", () => {
  it("renders a priced zero as a real $0.00", () => {
    expect(costDisplay(0, null, true, "en")).toEqual({
      kind: "value",
      text: "Est. cost $0.00",
      short: "Est. cost $0.00",
    });
  });

  it("renders a known estimate as a value", () => {
    expect(costDisplay(12.004, null, true, "en")).toEqual({
      kind: "value",
      text: "Est. cost $12.00",
      short: "Est. cost $12.00",
    });
  });

  it("reports missing model prices with the concrete reason", () => {
    expect(costDisplay(null, "missingModelPricing", true, "en")).toEqual({
      kind: "unavailable",
      text: "Est. cost unavailable — missing model prices",
      short: "Cost n/a ⓘ",
    });
    expect(costDisplay(null, "missingModelPricing", true, "zh-CN")).toEqual({
      kind: "unavailable",
      text: "预估成本不可用 — 缺少模型价格",
      short: "成本 n/a ⓘ",
    });
  });

  it("stays generic when usage exists without a known reason", () => {
    expect(costDisplay(null, null, true, "en")).toEqual({
      kind: "unavailable",
      text: "Est. cost unavailable",
      short: "Cost n/a ⓘ",
    });
  });

  it("marks a no-usage day as not applicable, never $0.00", () => {
    expect(costDisplay(null, null, false, "en")).toEqual({
      kind: "notApplicable",
      text: "Est. cost N/A — no usage",
      short: "Cost n/a ⓘ",
    });
    expect(costDisplay(null, null, false, "zh-CN")).toEqual({
      kind: "notApplicable",
      text: "预估成本不适用 — 无用量",
      short: "成本 n/a ⓘ",
    });
  });

  it("keeps the three states distinct even though the short mark is shared", () => {
    // A2 demotes the negative states' visual weight; the full copy (and the
    // statistics explainer) must still distinguish them.
    const missing = costDisplay(null, "missingModelPricing", true, "en");
    const noUsage = costDisplay(null, null, false, "en");
    expect(missing.kind).not.toBe(noUsage.kind);
    expect(missing.text).not.toBe(noUsage.text);
    expect(missing.short).toBe(noUsage.short);
  });

  it("treats a non-finite cost as unknown, not a value", () => {
    expect(costDisplay(Number.NaN, null, true, "en").kind).toBe("unavailable");
  });
});
