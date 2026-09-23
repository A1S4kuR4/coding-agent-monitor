import { describe, expect, it } from "vitest";

import { resolveLanguage, systemLanguage } from "./i18n";

describe("resolveLanguage", () => {
  it("maps every Chinese tag variant to Simplified Chinese", () => {
    expect(resolveLanguage("zh-CN")).toBe("zh-CN");
    expect(resolveLanguage("zh-TW")).toBe("zh-CN");
    expect(resolveLanguage("zh-Hans-CN")).toBe("zh-CN");
    expect(resolveLanguage("ZH")).toBe("zh-CN");
  });

  it("falls back to English for other and missing tags", () => {
    expect(resolveLanguage("en-US")).toBe("en");
    expect(resolveLanguage("en")).toBe("en");
    expect(resolveLanguage("de-DE")).toBe("en");
    expect(resolveLanguage("")).toBe("en");
    expect(resolveLanguage(null)).toBe("en");
    expect(resolveLanguage(undefined)).toBe("en");
  });
});

describe("systemLanguage", () => {
  it("never throws and returns a supported language", () => {
    expect(["en", "zh-CN"]).toContain(systemLanguage());
  });
});
