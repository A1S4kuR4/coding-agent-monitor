import type { CostUnknownReason } from "../../types/usage";
import type { Language } from "./i18n";
import { dictFor } from "./i18n";
import { formatUsd } from "./formatUsd";

export interface CostDisplay {
  /** "value": a complete estimated cost. "unavailable": usage exists but no
   * complete estimate can be computed. "notApplicable": no usage to price. */
  kind: "value" | "unavailable" | "notApplicable";
  /** Full display copy — the concrete reason for a non-value state. */
  text: string;
  /** Short slot label (review A2): identical to `text` for a priced value;
   * a quiet "n/a" mark for every other state. The full reason stays available
   * via the tooltip / "About these numbers". */
  short: string;
}

/** Projects a day's cost slot into user-visible copy. A real (priced) zero
 * stays `$0.00`; a missing price is "unavailable" with its concrete reason;
 * no usage is "N/A" — none of these ever collapse into the others. The short
 * form visually demotes the negative states (A2) while `text` keeps carrying
 * the distinction. */
export function costDisplay(
  costUsd: number | null,
  costUnknownReason: CostUnknownReason | null,
  hasUsage: boolean,
  lang: Language,
): CostDisplay {
  const d = dictFor(lang);
  const formatted = formatUsd(costUsd);
  if (formatted !== null) {
    return { kind: "value", text: d.costValue(formatted), short: d.costValue(formatted) };
  }
  if (hasUsage) {
    const text =
      costUnknownReason === "missingModelPricing"
        ? d.costMissingPrices
        : d.costUnavailable;
    return { kind: "unavailable", text, short: d.costNa };
  }
  return { kind: "notApplicable", text: d.costNoUsage, short: d.costNa };
}
