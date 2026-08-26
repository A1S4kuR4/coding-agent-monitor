import type { UsageSummary } from "../../types/usage";

/** The dashboard view machine. `ready` holds the last good summary plus the
 * current refresh and staleness flags; `error` shows for a failed first load. */
export type ViewState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | {
      status: "ready";
      summary: UsageSummary;
      refreshing: boolean;
      stale: boolean;
    };

export type ViewAction =
  | { type: "load-started" }
  | { type: "refresh-started" }
  | { type: "load-succeeded"; summary: UsageSummary }
  | { type: "load-failed"; keepExisting: boolean; message: string }
  | { type: "event-received"; summary: UsageSummary };

/** Pure state transitions for the dashboard. Kept in a reducer so the Phase 8
 * state machine (first-error, refresh success, stale degradation) is unit-tested
 * without a DOM, and so `App` effects never call setState synchronously. */
export function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "load-started":
      // Full-page retry from the error state.
      return { status: "loading" };

    case "refresh-started":
      // A background refresh only marks an already-ready dashboard as busy so
      // the header button disables; other states are untouched.
      return state.status === "ready"
        ? { ...state, refreshing: true }
        : state;

    case "load-succeeded":
      return {
        status: "ready",
        summary: action.summary,
        refreshing: false,
        stale: false,
      };

    case "load-failed":
      // With `keepExisting` a failure keeps the last good snapshot and marks it
      // stale; the first-ever load keeps false so it lands on the error page.
      return state.status === "ready" && action.keepExisting
        ? { ...state, refreshing: false, stale: true }
        : { status: "error", message: action.message };

    case "event-received":
      // A tray refresh emitted the same snapshot; apply it directly.
      return {
        status: "ready",
        summary: action.summary,
        refreshing: false,
        stale: false,
      };
  }
}