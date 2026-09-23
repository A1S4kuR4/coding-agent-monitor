import type { CSSProperties } from "react";
import { agentMeta } from "./agents";

import antigravityMark from "../../assets/agent-marks/antigravity.svg?raw";
import claudeCodeMark from "../../assets/agent-marks/claude-code.svg?raw";
import codexMark from "../../assets/agent-marks/codex.svg?raw";
import opencodeMark from "../../assets/agent-marks/opencode.svg?raw";
import othersMark from "../../assets/agent-marks/others.svg?raw";
import unknownMark from "../../assets/agent-marks/unknown.svg?raw";

/**
 * Agent sigils (marks doc §2): the redrawn, stroke-only official marks from
 * `src/assets/agent-marks/*.svg`, inlined verbatim so `stroke="currentColor"`
 * inherits the agent colour token through `--mark-color` — colour values stay
 * in App.css, never in components. The span is decorative (`aria-hidden`);
 * the accessible label is always the agent's name.
 *
 * Identity shape map: spark / `>_` / arch / square / diamond / plus — six
 * silhouettes distinguishable at 15px without colour. The display-only
 * "others" aggregate and the overflow entry share the plus; unknown agents
 * (and agents without a drawn mark) share the neutral diamond.
 */
const SIGIL_ASSETS: Record<string, string> = {
  "claude-code": claudeCodeMark,
  codex: codexMark,
  antigravity: antigravityMark,
  opencode: opencodeMark,
  unknown: unknownMark,
  others: othersMark,
};

/** The asset key for an agent id: its pinned sigil, the plus for the
 * display-only "others" group, or the neutral diamond for anything without a
 * drawn mark. */
function sigilAssetFor(id: string): string {
  if (id === "others") return "others";
  return agentMeta(id).sigil ?? "unknown";
}

export function Sigil({
  id,
  colorVar,
}: {
  id: string;
  /** Readable CSS custom-property name, e.g. "--agent-codex". */
  colorVar: string;
}) {
  return (
    <span
      className="sigil"
      aria-hidden="true"
      style={{ "--mark-color": `var(${colorVar})` } as CSSProperties}
      dangerouslySetInnerHTML={{ __html: SIGIL_ASSETS[sigilAssetFor(id)] ?? unknownMark }}
    />
  );
}
