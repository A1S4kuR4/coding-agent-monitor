import { invoke } from "@tauri-apps/api/core";
import type {
  RefreshTrigger,
  UsageCollectionState,
} from "../types/usage";

export function fetchUsageState(): Promise<UsageCollectionState> {
  return invoke<UsageCollectionState>("get_usage_state");
}

export function refreshUsageState(
  trigger: RefreshTrigger,
): Promise<UsageCollectionState> {
  return invoke<UsageCollectionState>("refresh_usage_state", { trigger });
}
