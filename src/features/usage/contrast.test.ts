import { describe, expect, it } from "vitest";

import css from "../../App.css?raw";

/**
 * WCAG AA contrast regression for the theme tokens in App.css (T03).
 *
 * The ratios below were measured against the current stylesheet on 2026-09-06.
 * Light paper #faf6f0: ink 15.83, ink-2 7.01, ink-3 4.85, terracotta 5.02,
 * indigo-focus 6.64, stale pair 6.04, white-on-accent 5.40. Dark paper
 * #191612: ink 14.53, ink-2 7.78, ink-3 7.38, terracotta 6.15, indigo-focus
 * 8.52, stale pair 8.81, white-on-accent 4.79. All pass AA (>= 4.5 for body
 * and small text, >= 3 for the focus indicator as a non-text cue).
 *
 * The former light-theme delta colours (red 3.13 / green 2.46 / grey 2.64)
 * failed AA; deltas are now neutral ink, and this test guards against a
 * token regression ever reintroducing a failing text colour.
 */

function tokensFrom(block: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const [, name, value] of block.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) {
    map.set(name, value);
  }
  return map;
}

const darkIndex = css.indexOf("prefers-color-scheme: dark");
if (darkIndex === -1) throw new Error("dark theme block not found in App.css");

const light = tokensFrom(css.slice(0, darkIndex));
const darkOverrides = tokensFrom(css.slice(darkIndex));
// The dark theme redefines a token by overriding it on :root inside the media
// query; unresolved tokens inherit the light value.
const dark = new Map([...light, ...darkOverrides]);

function luminance(hex: string): number {
  const channels = [0, 2, 4].map((i) => parseInt(hex.slice(1 + i, 3 + i), 16) / 255);
  const [r, g, b] = channels.map((v) =>
    v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4),
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG 2.x contrast ratio between two hex colours. */
export function contrast(a: string, b: string): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

/** Text pairs that must meet WCAG AA for normal text (>= 4.5:1). */
const TEXT_PAIRS: { fg: string; bg: string }[] = [
  { fg: "ink", bg: "paper" },
  { fg: "ink-2", bg: "paper" },
  { fg: "ink-3", bg: "paper" },
  { fg: "terracotta", bg: "paper" },
  { fg: "stale-ink", bg: "stale-bg" },
  { fg: "ink-2", bg: "stale-bg" },
  { fg: "ink-3", bg: "surface-tooltip" },
  { fg: "ink", bg: "surface-tooltip" },
];

describe("App.css theme tokens meet WCAG AA", () => {
  for (const [theme, tokens] of [
    ["light", light],
    ["dark", dark],
  ] as const) {
    it(`${theme}: body and small text pairs are >= 4.5:1`, () => {
      expect(TEXT_PAIRS.length).toBeGreaterThan(0);
      for (const { fg, bg } of TEXT_PAIRS) {
        const fgValue = tokens.get(fg);
        const bgValue = tokens.get(bg);
        expect(fgValue, `token --${fg} missing in ${theme}`).toBeTruthy();
        expect(bgValue, `token --${bg} missing in ${theme}`).toBeTruthy();
        const ratio = contrast(fgValue!, bgValue!);
        expect(
          ratio,
          `${theme} --${fg} on --${bg} is ${ratio.toFixed(2)}:1`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    });

    it(`${theme}: white on the accent button fill is >= 4.5:1`, () => {
      const ratio = contrast("#ffffff", tokens.get("accent-fill")!);
      expect(ratio).toBeGreaterThanOrEqual(4.5);
    });

    it(`${theme}: the focus ring is >= 3:1 against the paper (non-text cue)`, () => {
      const ratio = contrast(tokens.get("indigo-focus")!, tokens.get("paper")!);
      expect(ratio).toBeGreaterThanOrEqual(3);
    });
  }

  it("no delta direction token reintroduces a judged-good/bad colour", () => {
    expect(light.has("delta-up")).toBe(false);
    expect(light.has("delta-down")).toBe(false);
    expect(light.has("delta-flat")).toBe(false);
  });
});
