import type { UsageCollectionState } from "../../types/usage";

/** Stable failure category for the first-failure error page. The English/Chinese
 * copy is chosen by the App from the language dictionary — the reducer never
 * stores user-facing text. */
export type ViewStateErrorReason = "timedOut" | "cancelled" | "failed" | "transport";

export type ViewState =
  | { status: "loading"; collection: UsageCollectionState | null }
  | { status: "error"; reason: ViewStateErrorReason; collection: UsageCollectionState | null }
  | { status: "ready"; collection: UsageCollectionState };

export type ViewAction =
  | { type: "state-received"; state: UsageCollectionState }
  | { type: "transport-failed" };

export const initialViewState: ViewState = {
  status: "loading",
  collection: null,
};

function failureReason(state: UsageCollectionState): ViewStateErrorReason {
  switch (state.lastAttempt?.failure) {
    case "timedOut":
      return "timedOut";
    case "cancelled":
      return "cancelled";
    default:
      return "failed";
  }
}

function project(state: UsageCollectionState): ViewState {
  if (state.snapshot) return { status: "ready", collection: state };
  if (state.refreshing || state.lastAttempt?.outcome === "inProgress") {
    return { status: "loading", collection: state };
  }
  if (state.lastAttempt?.outcome === "failed") {
    return {
      status: "error",
      reason: failureReason(state),
      collection: state,
    };
  }
  return { status: "loading", collection: state };
}

function shouldApply(
  current: UsageCollectionState | null,
  incoming: UsageCollectionState,
): boolean {
  if (!current) return true;
  if (incoming.revision !== current.revision) {
    return incoming.revision > current.revision;
  }
  // A read can re-evaluate freshness without changing the transition revision.
  return incoming.freshness.checkedAt >= current.freshness.checkedAt;
}

/** Applies the one Rust-owned state envelope. Revisions make event/command
 * ordering harmless: a late initialization read or refresh response cannot
 * replace a newer success, failure, or in-progress transition. */
export function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "state-received":
      return shouldApply(state.collection, action.state)
        ? project(action.state)
        : state;
    case "transport-failed":
      return state.status === "ready"
        ? state
        : {
            status: "error",
            reason: "transport",
            collection: state.collection,
          };
  }
}

export function needsRefresh(state: UsageCollectionState): boolean {
  return !state.refreshing && (!state.snapshot || state.freshness.status !== "fresh");
}
