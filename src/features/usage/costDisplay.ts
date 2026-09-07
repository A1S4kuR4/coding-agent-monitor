import type { CostUnknownReason } from "../../types/usage";
import type { Language } from "./i18n";
import { dictFor } from "./i18n";
import { formatUsd } from "./formatUsd";

export interface CostDisplay {
  /** "value": a complete estimated cost. "unavailable": usage exists but no
   * complete estimate can be computed. "notApplicable": no usage to price. */
  kind: "value" | "unavailable" | "notApplicable";
  text: string;
}

/** Projects a day's cost slot into user-visible copy. A real (priced) zero
 * stays `$0.00`; a missing price is "unavailable" with its concrete reason;
 * no usage is "N/A" — none of these ever collapse into the others. */
export function costDisplay(
  costUsd: number | null,
  costUnknownReason: CostUnknownReason | null,
  hasUsage: boolean,
  lang: Language,
): CostDisplay {
  const d = dictFor(lang);
  const formatted = formatUsd(costUsd);
  if (formatted !== null) {
    return { kind: "value", text: d.costValue(formatted) };
  }
  if (hasUsage) {
    return {
      kind: "unavailable",
      text:
        costUnknownReason === "missingModelPricing"
          ? d.costMissingPrices
          : d.costUnavailable,
    };
  }
  return { kind: "notApplicable", text: d.costNoUsage };
}
