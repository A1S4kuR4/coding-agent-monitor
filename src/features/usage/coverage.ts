import type { CoverageInfo } from "../../types/usage";
import type { Language } from "./i18n";
import { dictFor } from "./i18n";

/** Renders coverage metadata as one bounded status sentence, or "" when the
 * coverage is complete (or malformed/empty — never invented text). Copy stays
 * action-oriented and safe: no paths, no raw errors — only what happened. */
export function coverageText(coverage: CoverageInfo, lang: Language): string {
  if (coverage.status !== "possiblyIncomplete" || coverage.diagnostics.length === 0) {
    return "";
  }
  const d = dictFor(lang);
  const parts = coverage.diagnostics.map((diagnostic) => {
    const label = d.coverageKinds[diagnostic.kind];
    const detail =
      diagnostic.count === 1
        ? label.one
        : `${diagnostic.count} ${label.many}`;
    return `${diagnostic.agentDisplayName}: ${detail}`;
  });
  return `${d.coveragePrefix}${parts.join("; ")}${d.coverageSuffix}`;
}
