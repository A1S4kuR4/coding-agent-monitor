import { invoke } from "@tauri-apps/api/core";
import type { UsageSummary } from "../types/usage";

export function fetchUsageSummary(): Promise<UsageSummary> {
  return invoke<UsageSummary>("get_usage_summary");
}
