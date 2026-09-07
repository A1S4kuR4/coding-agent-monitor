import { describe, expect, it } from "vitest";

import { costDisplay } from "./costDisplay";

describe("costDisplay", () => {
  it("renders a priced zero as a real $0.00", () => {
    expect(costDisplay(0, null, true, "en")).toEqual({
      kind: "value",
      text: "Est. cost $0.00",
    });
  });

  it("renders a known estimate as a value", () => {
    expect(costDisplay(12.004, null, true, "en")).toEqual({
      kind: "value",
      text: "Est. cost $12.00",
    });
  });

  it("reports missing model prices with the concrete reason", () => {
    expect(costDisplay(null, "missingModelPricing", true, "en")).toEqual({
      kind: "unavailable",
      text: "Est. cost unavailable — missing model prices",
    });
    expect(costDisplay(null, "missingModelPricing", true, "zh-CN")).toEqual({
      kind: "unavailable",
      text: "预估成本不可用 — 缺少模型价格",
    });
  });

  it("stays generic when usage exists without a known reason", () => {
    expect(costDisplay(null, null, true, "en")).toEqual({
      kind: "unavailable",
      text: "Est. cost unavailable",
    });
  });

  it("marks a no-usage day as not applicable, never $0.00", () => {
    expect(costDisplay(null, null, false, "en")).toEqual({
      kind: "notApplicable",
      text: "Est. cost N/A — no usage",
    });
    expect(costDisplay(null, null, false, "zh-CN")).toEqual({
      kind: "notApplicable",
      text: "预估成本不适用 — 无用量",
    });
  });

  it("treats a non-finite cost as unknown, not a value", () => {
    expect(costDisplay(Number.NaN, null, true, "en").kind).toBe("unavailable");
  });
});
