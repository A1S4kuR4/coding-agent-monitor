import { describe, expect, it } from "vitest";

import { relativeTime } from "./relativeTime";

const base = "2026-08-25T10:00:00Z";
const at = (offsetSeconds: number) =>
  relativeTime(base, new Date(Date.parse(base) + offsetSeconds * 1000));

describe("relativeTime", () => {
  it("shows just now for zero and sub-minute ages", () => {
    expect(at(0)).toBe("just now");
    expect(at(59)).toBe("just now");
  });

  it("switches to minutes at the 60s boundary", () => {
    expect(at(60)).toBe("1m ago");
    expect(at(119)).toBe("1m ago");
    expect(at(120)).toBe("2m ago");
    expect(at(3599)).toBe("59m ago");
  });

  it("switches to hours at the 3600s boundary", () => {
    expect(at(3600)).toBe("1h ago");
    expect(at(3600 * 23 + 59 * 60)).toBe("23h ago");
  });

  it("switches to days past 24 hours", () => {
    expect(at(3600 * 24)).toBe("1d ago");
    expect(at(3600 * 24 * 3 + 61)).toBe("3d ago");
  });

  it("treats a future or unparseable timestamp as just now", () => {
    expect(at(-5)).toBe("just now");
    expect(relativeTime("not-a-date", new Date())).toBe("just now");
  });
});