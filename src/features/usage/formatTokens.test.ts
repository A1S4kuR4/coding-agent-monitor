import { describe, expect, it } from "vitest";
import { formatTokens } from "./formatTokens";

describe("formatTokens", () => {
  it("formats the dashboard examples", () => {
    expect(formatTokens(13_590_000)).toBe("13.59M");
    expect(formatTokens(8_420_000)).toBe("8.42M");
  });

  it("keeps small values readable", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(1_200)).toBe("1.2K");
  });
});
