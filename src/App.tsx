import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import { fetchUsageHistory, fetchUsageState, refreshUsageState } from "./lib/usage-api";
import {
  getPreferences,
  hideMainWindow,
  updatePreferences,
} from "./lib/preferences-api";
import type {
  AppPreferences,
  LanguagePreference,
  PreferencesPatch,
} from "./types/preferences";
import type {
  DailyUsage,
  HistoryUsage,
  RefreshTrigger,
  TokenBreakdown,
  UsageCollectionState,
} from "./types/usage";
import { formatTokens } from "./features/usage/formatTokens";
import { activeAgentRows } from "./features/usage/usageRows";
import {
  initialViewState,
  needsRefresh,
  viewReducer,
  type ViewStateErrorReason,
} from "./features/usage/viewState";
import { formatPercent } from "./features/usage/formatPercent";
import { costDisplay } from "./features/usage/costDisplay";
import { coverageText } from "./features/usage/coverage";
import { cacheInputShare } from "./features/usage/cacheInputShare";
import { relativeTime } from "./features/usage/relativeTime";
import { agentMark, agentMeta, compareByMeta, sortAgents } from "./features/usage/agents";
import { formatDelta, type DeltaBasis } from "./features/usage/formatDelta";
import {
  buildAllChart,
  buildAgentChart,
  dayValue,
  type AllChartGrouping,
} from "./features/usage/chartView";
import {
  effectiveAgentFilter,
  visibleFilterAgents,
} from "./features/usage/trendFilter";
import { allDayAriaLabel, agentDayAriaLabel, fullDate } from "./features/usage/tooltipLabel";
import { dictFor, systemLanguage, type Dict, type Language } from "./features/usage/i18n";

function shortDate(date: string): string {
  return date.slice(5).replace("-", "/");
}

const deltaClass = (kind: string) => `delta ${kind}`;

function errorCopy(d: Dict, reason: ViewStateErrorReason): string {
  switch (reason) {
    case "timedOut":
      return d.errorTimedOut;
    case "cancelled":
      return d.errorCancelled;
    case "failed":
      return d.errorFailed;
    case "transport":
      return d.errorTransport;
  }
}

/** Today's token composition by type, each with its count and share of the day.
 * Input, Output and Cache read always render; optional known types and the
 * explicit unclassified fallback render only when they carry tokens. */
function BreakdownList({
  total,
  breakdown,
  d,
}: {
  total: number;
  breakdown: TokenBreakdown;
  d: Dict;
}) {
  const parts: { key: string; label: string; value: number }[] = [
    { key: "input", label: d.breakdown.input, value: breakdown.inputTokens },
    { key: "output", label: d.breakdown.output, value: breakdown.outputTokens },
    { key: "cacheRead", label: d.breakdown.cacheRead, value: breakdown.cacheReadTokens },
  ];
  if (breakdown.cacheCreationTokens > 0) {
    parts.push({
      key: "cacheCreation",
      label: d.breakdown.cacheCreation,
      value: breakdown.cacheCreationTokens,
    });
  }
  if (breakdown.reasoningTokens > 0) {
    parts.push({
      key: "reasoning",
      label: d.breakdown.reasoning,
      value: breakdown.reasoningTokens,
    });
  }
  if (breakdown.unclassifiedTokens > 0) {
    parts.push({
      key: "unclassified",
      label: d.breakdown.unclassified,
      value: breakdown.unclassifiedTokens,
    });
  }

  return (
    <dl className="breakdown-list">
      {parts.map((part) => {
        const pct = total > 0 ? Math.round((part.value / total) * 100) : 0;
        // The 2px proportion bar under each row uses the unrounded share, so it
        // never shows a rounded 0% bar for a tiny-but-real slice. It is rendered
        // by a CSS pseudo-element (aria-hidden by construction), keeping the
        // definition list's dt+dd-only children valid; the text % stays the
        // accessible source of truth.
        const barWidth = total > 0 ? (part.value / total) * 100 : 0;
        return (
          <div
            className="breakdown-row"
            key={part.key}
            style={{ "--bar-width": `${barWidth}%` } as CSSProperties}
          >
            <dt>{part.label}</dt>
            <dd>
              <span className="breakdown-count" title={part.value.toLocaleString()}>
                {formatTokens(part.value)}
              </span>
              <span className="breakdown-pct">{pct}%</span>
            </dd>
          </div>
        );
      })}
    </dl>
  );
}

/** The stable identity disc shared by list rows, filter chips, tooltip legends
 * and the day-detail panel: an agent-coloured ring with the deterministic
 * agentMark monogram in ordinary ink. Colour alone never has to identify an
 * agent — unknown agents share one neutral token, so the glyph carries identity
 * and the ring only echoes it. The glyph is decorative; the name is the
 * accessible label. */
function AgentMark({ colorVar, glyph }: { colorVar: string; glyph: string }) {
  return (
    <span
      className="agent-mark"
      aria-hidden="true"
      style={{ "--mark-color": `var(${colorVar})` } as CSSProperties}
    >
      {glyph}
    </span>
  );
}

/** Full composition of one trend day, shared verbatim by the hover/focus
 * tooltip and the click-pinned detail panel. In All mode it always lists every
 * contributing agent — the chart's "others" group is display-only and never
 * hides members here; its aggregate row is clearly labelled. */
function DayDetailContent({
  day,
  prevDay,
  filter,
  filterName,
  grouping,
  d,
  lang,
  basis,
}: {
  day: DailyUsage;
  prevDay: DailyUsage | undefined;
  filter: string | null;
  filterName: string | null;
  grouping: AllChartGrouping;
  d: Dict;
  lang: Language;
  basis: DeltaBasis;
}) {
  if (filter !== null) {
    const sel = day.agents.find((a) => a.id === filter);
    const selValue = sel?.tokens ?? 0;
    const selPrev = prevDay
      ? (prevDay.agents.find((a) => a.id === filter)?.tokens ?? 0)
      : undefined;
    const selDelta = formatDelta(selValue, selPrev, lang, basis);
    const share = day.totalTokens > 0 ? (selValue / day.totalTokens) * 100 : 0;
    return (
      <>
        <p className="tooltip-date">{fullDate(lang, day.date)}</p>
        <p className="tooltip-total">
          {filterName}: {formatTokens(selValue)}
        </p>
        <p className="tooltip-share">{d.tooltipOfDay(share.toFixed(1))}</p>
        {selDelta?.label && (
          <p className={`tooltip-delta ${deltaClass(selDelta.kind)}`}>{selDelta.label}</p>
        )}
      </>
    );
  }

  const delta = formatDelta(day.totalTokens, prevDay?.totalTokens, lang, basis);
  const members = new Set(grouping.memberIds);
  const others = day.agents.filter((a) => a.tokens > 0 && !members.has(a.id));
  const othersTokens = others.reduce((sum, a) => sum + a.tokens, 0);
  const othersPct =
    day.totalTokens > 0 ? (othersTokens / day.totalTokens) * 100 : 0;
  return (
    <>
      <p className="tooltip-date">{fullDate(lang, day.date)}</p>
      <p className="tooltip-total">
        {d.tooltipTokensTotal(formatTokens(day.totalTokens))}
      </p>
      <ul className="tooltip-agents">
        {grouping.hasOthers && othersTokens > 0 && (
          <li>
            <AgentMark colorVar="--agent-others" glyph="+" />
            <span className="tooltip-agent-name">{d.otherAgents(others.length)}</span>
            <span className="tooltip-agent-value">
              {formatTokens(othersTokens)} · {othersPct.toFixed(1)}%
            </span>
          </li>
        )}
        {sortAgents(day.agents).map((a) => {
          const pct = day.totalTokens > 0 ? (a.tokens / day.totalTokens) * 100 : 0;
          return (
            <li key={a.id}>
              <AgentMark colorVar={agentMeta(a.id).colorVar} glyph={agentMark(a.displayName)} />
              <span className="tooltip-agent-name">{a.displayName}</span>
              <span className="tooltip-agent-value">
                {formatTokens(a.tokens)} · {pct.toFixed(1)}%
              </span>
            </li>
          );
        })}
      </ul>
      {delta.label && (
        <p className={`tooltip-delta ${deltaClass(delta.kind)}`}>{delta.label}</p>
      )}
    </>
  );
}

function App() {
  // The language resolves once per mount from the system (v0.4 plan §4.5);
  // an explicit persisted choice (T05) overrides it when the preference
  // arrives. Every visible string, date and aria label uses the same
  // resolved language.
  const [lang, setLang] = useState<Language>(() => systemLanguage());
  const d = dictFor(lang);
  // The view machine lives in a pure reducer so its transitions are unit-tested
  // and no effect ever calls setState synchronously.
  const [view, dispatch] = useReducer(viewReducer, initialViewState);
  // Persisted preferences (T05). `null` means not loaded (or unavailable);
  // the settings section only renders when they are known.
  const [prefs, setPrefs] = useState<AppPreferences | null>(null);
  const [settingsError, setSettingsError] = useState(false);
  // The first-close tray explanation, opened by a Rust event when the user
  // closes the window before acknowledging it once.
  const [closeNoticeOpen, setCloseNoticeOpen] = useState(false);
  // Clock sampling happens in effects/events, never during rendering.
  const [now, setTick] = useState(() => Date.now());
  // Which agent ids currently have their per-model breakdown expanded.
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  // The seven-day trend filter: `null` means All agents, otherwise an agent id.
  // The raw selection is resolved against the visible range every render (see
  // activeFilter), so a vanished agent deterministically falls back to All.
  const [agentFilter, setAgentFilter] = useState<string | null>(null);
  // The click-pinned trend day, tracked by ISO date so refreshes and window
  // shifts can never misalign the detail: a date that leaves the visible range
  // simply stops matching and the panel disappears deterministically.
  const [selectedDay, setSelectedDay] = useState<string | null>(null);
  const [historyRange, setHistoryRange] = useState<7 | 30>(7);
  const [history, setHistory] = useState<HistoryUsage | null>(null);
  const [historyStatus, setHistoryStatus] = useState<"idle" | "loading" | "failed">("idle");
  const historyRequest = useRef(0);
  const chooseRange = (days: 7 | 30) => {
    const identity = ++historyRequest.current;
    setHistoryRange(days);
    setHistory(null);
    setActiveDay(null);
    setSelectedDay(null);
    setHistoryStatus(days === 7 ? "idle" : "loading");
    if (days === 7) return;
    void fetchUsageHistory(days).then((result) => {
      if (!mounted.current || identity !== historyRequest.current) return;
      if (result.days.length !== days) { setHistoryStatus("failed"); return; }
      setTick(Date.now());
      setHistory(result);
      setHistoryStatus("idle");
    }, () => {
      if (mounted.current && identity === historyRequest.current) setHistoryStatus("failed");
    });
  };

  // Toolkit for the per-day bar: which day (index) is active, and its computed
  // fixed-position placement. `null` hides the tooltip.
  const [activeDay, setActiveDay] = useState<number | null>(null);
  const [tooltipPos, setTooltipPos] = useState<{ left: number; top: number } | null>(
    null,
  );
  const dayEls = useRef<Array<HTMLElement | null>>([]);
  const tooltipEl = useRef<HTMLDivElement | null>(null);
  const moreMenuRef = useRef<HTMLDetailsElement | null>(null);
  const moreMenuSummaryRef = useRef<HTMLElement | null>(null);
  // Holds the async-returned unlisten functions so cleanup can release them even
  // when registration resolved after an unmount.
  const clientRefreshInFlight = useRef(false);
  const mounted = useRef(true);
  const focusUnlisten = useRef<(() => void) | undefined>(undefined);
  const trayUnlisten = useRef<(() => void) | undefined>(undefined);
  const noticeUnlisten = useRef<(() => void) | undefined>(undefined);

  const runRefresh = useCallback((trigger: RefreshTrigger) => {
    if (clientRefreshInFlight.current) return;
    clientRefreshInFlight.current = true;
    void refreshUsageState(trigger)
      .then(
        (state) => {
          if (mounted.current) dispatch({ type: "state-received", state });
        },
        () => {
          if (mounted.current) dispatch({ type: "transport-failed" });
        },
      )
      .finally(() => {
        clientRefreshInFlight.current = false;
      });
  }, []);

  useEffect(() => () => {
    mounted.current = false;
  }, []);

  // Refetch when the window regains focus (it is never remounted when hidden to
  // the tray, so a mount-time fetch would otherwise show stale numbers). The
  // listener registration is async: cleanup handles both an already-resolved
  // unlisten and one that lands after unmount.
  useEffect(() => {
    let active = true;
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (!active || !focused) return;
        runRefresh("focus");
      })
      .then(
        (fn) => {
          if (active) {
            focusUnlisten.current = fn;
          } else {
            // Unmounted while registration was in flight: release it right away
            // so the webview listener never leaks.
            fn();
          }
        },
        () => undefined,
      );
    return () => {
      active = false;
      focusUnlisten.current?.();
      focusUnlisten.current = undefined;
    };
  }, [runRefresh]);

  // Subscribe before reading current state. The revisioned reducer makes an
  // event/read race harmless, while the read recovers events missed before the
  // window existed. A stale/no-snapshot read triggers one safe refresh.
  useEffect(() => {
    let active = true;
    // Starting registration before the read closes the usual missed-event
    // window; the read does not wait for the async unlisten handle to resolve.
    void listen<UsageCollectionState>(
      "usage-state-updated",
      (event) => {
        if (active) {
          dispatch({ type: "state-received", state: event.payload });
        }
      },
    ).then(
      (fn) => {
        if (active) {
          trayUnlisten.current = fn;
        } else {
          fn();
        }
      },
      () => {
        // The state read below still gives a recoverable first view. A native
        // listener failure is not surfaced as a raw error string.
      },
    );

    void fetchUsageState().then(
      (state) => {
        if (!active) return;
        dispatch({ type: "state-received", state });
        if (needsRefresh(state)) runRefresh("startup");
      },
      () => {
        if (active) dispatch({ type: "transport-failed" });
      },
    );
    return () => {
      active = false;
      trayUnlisten.current?.();
      trayUnlisten.current = undefined;
    };
  }, [runRefresh]);

  // Refresh only the relative-time wording on a timer; this never starts a
  // worker, so the label can tick without extra collection.
  useEffect(() => {
    const id = window.setInterval(() => setTick(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, [setTick]);

  // Load the persisted preferences once and listen for the Rust first-close
  // event. An explicit language choice overrides the system resolution; a
  // preference failure degrades quietly to system language with no settings
  // section — usage data never depends on preferences.
  useEffect(() => {
    let active = true;
    void getPreferences().then(
      (loaded) => {
        if (!active) return;
        setPrefs(loaded);
        if (loaded.language !== "system") setLang(loaded.language);
      },
      () => undefined,
    );
    void listen("close-notice-requested", () => {
      if (active) setCloseNoticeOpen(true);
    }).then(
      (fn) => {
        if (active) {
          noticeUnlisten.current = fn;
        } else {
          fn();
        }
      },
      () => undefined,
    );
    return () => {
      active = false;
      noticeUnlisten.current?.();
      noticeUnlisten.current = undefined;
    };
  }, []);

  // Keep the fixed-position tooltip inside the viewport. Runs after commit so
  // the tooltip is measured before being placed. Synchronous setState here is
  // intentional (measure-and-position), so the lint guard is scoped to this
  // block only.
  // Block only: the measurement below sits a floating tooltip and intentionally
  // writes tooltipPos right after render (and again on resize/scroll/relayout).
  /* eslint-disable react-hooks/set-state-in-effect */
  const positionTooltip = useCallback(() => {
    const day = activeDay;
    if (day === null) {
      setTooltipPos(null);
      return;
    }
    const dayEl = dayEls.current[day];
    const tipEl = tooltipEl.current;
    if (!dayEl || !tipEl) return;
    const dayRect = dayEl.getBoundingClientRect();
    const tipRect = tipEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const margin = 8;
    // Prefer sitting beside the bar (right, then left) so it rarely covers its
    // own bar, the filters or the Total label; fall back to above/below only
    // when no side fits.
    let left = dayRect.right + margin;
    let top = dayRect.top + dayRect.height / 2 - tipRect.height / 2;
    if (left + tipRect.width > vw - margin) {
      left = dayRect.left - margin - tipRect.width;
    }
    if (left < margin) {
      left = dayRect.left + dayRect.width / 2 - tipRect.width / 2;
      top = dayRect.top - tipRect.height - margin;
      if (top < margin) top = dayRect.bottom + margin;
    }
    // Final hard clamp so the fixed overlay can't leave the viewport.
    left = Math.max(margin, Math.min(left, vw - tipRect.width - margin));
    top = Math.max(margin, Math.min(top, vh - tipRect.height - margin));
    setTooltipPos({ left, top });
  }, [activeDay]);

  // (Re)measure whenever the open day / series changes and keep the overlay
  // inside the viewport across window resizes and scrolling — a position:fixed
  // element stays put geometrically, so a relayout must re-clamp it.
  useLayoutEffect(() => {
    positionTooltip();
    window.addEventListener("resize", positionTooltip);
    window.addEventListener("scroll", positionTooltip, true);
    return () => {
      window.removeEventListener("resize", positionTooltip);
      window.removeEventListener("scroll", positionTooltip, true);
    };
  }, [positionTooltip, agentFilter, selectedDay]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const retry = () => runRefresh("manual");

  const toggleAgent = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const manualRefresh = () => {
    runRefresh("manual");
    if (historyRange === 30) chooseRange(30);
  };

  // Optimistic preference update with an honest revert: the Rust command
  // applies its OS effects before persisting, so a rejection means nothing
  // was recorded and the control returns to the persisted value.
  const changePrefs = async (patch: PreferencesPatch) => {
    if (!prefs) return;
    const previous = prefs;
    setSettingsError(false);
    setPrefs({ ...prefs, ...patch });
    try {
      const saved = await updatePreferences(patch);
      if (mounted.current) setPrefs(saved);
    } catch {
      if (mounted.current) {
        setPrefs(previous);
        setSettingsError(true);
      }
    }
  };

  // Runtime language switching is allowed (v0.4 plan §4.5); the Rust side
  // re-resolves the tray language from the same preference, so the tray and
  // the window stay in one language.
  const changeLanguage = (preference: LanguagePreference) => {
    setLang(preference === "system" ? systemLanguage() : preference);
    void changePrefs({ language: preference });
  };

  const acknowledgeCloseNotice = () => {
    setCloseNoticeOpen(false);
    void updatePreferences({ closeNoticeAcknowledged: true }).then(
      (saved) => {
        if (mounted.current) setPrefs(saved);
      },
      () => undefined,
    );
  };

  const hideCloseNoticeOnce = () => {
    setCloseNoticeOpen(false);
    void hideMainWindow().catch(() => undefined);
  };

  // The first-close explanation must be reachable from every view state
  // (loading, error, dashboard) — the user can close the window during any.
  const closeNotice = closeNoticeOpen ? (
    <div className="close-notice" role="alert">
      <p className="close-notice-title">{d.closeNoticeTitle}</p>
      <p>{d.closeNoticeBody}</p>
      <div className="close-notice-actions">
        <button type="button" onClick={acknowledgeCloseNotice}>
          {d.closeNoticeAcknowledge}
        </button>
        <button
          type="button"
          className="notice-secondary"
          onClick={hideCloseNoticeOnce}
        >
          {d.closeNoticeHideOnce}
        </button>
      </div>
    </div>
  ) : null;

  if (view.status === "loading") {
    return (
      <main className="shell status-panel" aria-live="polite">
        {closeNotice}
        <p className="eyebrow">Coding Agent Monitor</p>
        <h1>{d.loadingTitle}</h1>
      </main>
    );
  }

  if (view.status === "error") {
    return (
      <main className="shell status-panel" role="alert">
        {closeNotice}
        <p className="eyebrow">Coding Agent Monitor</p>
        <h1>{d.errorTitle}</h1>
        <p className="status-copy">{errorCopy(d, view.reason)}</p>
        <p className="status-copy">{d.errorHint}</p>
        <button type="button" onClick={retry}>
          {d.tryAgain}
        </button>
      </main>
    );
  }

  const { collection } = view;
  const snapshot = collection.snapshot;
  if (!snapshot) return null;
  const { summary } = snapshot;
  const historyDays = historyRange === 7 ? summary.last7Days : (history?.days ?? []);
  const historyScope = historyRange === 7 ? snapshot.scope : history?.scope;
  const historyAge = history ? now - Date.parse(history.collectedAt) : 0;
  const historyExpired = !Number.isFinite(historyAge) || historyAge < 0 || historyAge > 600_000;
  const historyIsCurrent = historyScope?.endDate === collection.freshness.currentDate &&
    historyScope?.timeZone === collection.freshness.currentTimeZone;

  const isCurrentScope =
    snapshot.scope.endDate === collection.freshness.currentDate &&
    snapshot.scope.timeZone === collection.freshness.currentTimeZone;
  const refreshFailed = collection.lastAttempt?.outcome === "failed";
  const stale = collection.freshness.status !== "fresh";
  const todayAgents = activeAgentRows(summary.today.agents);
  // Cost slot: a priced zero stays $0.00, a missing price shows why the
  // estimate is unavailable, and no usage is N/A — three distinct states.
  const cost = costDisplay(
    summary.today.estimatedCostUsd,
    summary.today.costUnknownReason,
    summary.today.totalTokens > 0,
    lang,
  );
  const shareText = formatPercent(summary.today.cacheReadShare);
  const updatedText = relativeTime(summary.collectedAt, new Date(), lang);

  // Recognized agents across the window (id -> display name, first-seen wins),
  // then ordered by the canonical fixed metadata so the chips read consistently.
  const recognized: { id: string; displayName: string }[] = [];
  const seen = new Set<string>();
  for (const agent of summary.today.agents) {
    if (!seen.has(agent.id)) {
      seen.add(agent.id);
      recognized.push({ id: agent.id, displayName: agent.displayName });
    }
  }
  for (const day of historyDays) {
    for (const agent of day.agents) {
      if (!seen.has(agent.id)) {
        seen.add(agent.id);
        recognized.push({ id: agent.id, displayName: agent.displayName });
      }
    }
  }
  recognized.sort(compareByMeta);

  // The seven-day chart honours the agent filter. The raw selection resolves
  // against the visible range first: if the selected agent has disappeared the
  // filter falls back to All deterministically (and its chip renders as such).
  // `chartDays` is the pure view-model (stacked vs single-agent); `trendSeries`
  // feeds the per-day axis labels and the "Total" aggregate.
  const activeFilter = effectiveAgentFilter(agentFilter, historyDays);
  const allChart = buildAllChart(historyDays);
  const chartDays =
    activeFilter === null
      ? allChart.days
      : buildAgentChart(historyDays, activeFilter);
  const trendSeries = historyDays.map((day) => dayValue(day, activeFilter));
  const trendTotal = trendSeries.reduce((sum, value) => sum + BigInt(value), 0n);

  // Trend filter chips: canonically ordered agents, capped inline with the
  // overflow behind a keyboard-operable "More agents" disclosure. The effective
  // (not raw) selection drives the swap so a vanished agent never pins a chip.
  const chipLayout = visibleFilterAgents(recognized, activeFilter);
  const activeFilterName =
    activeFilter !== null
      ? (recognized.find((a) => a.id === activeFilter)?.displayName ??
        agentMeta(activeFilter).displayName)
      : null;

  // Direction of a day-over-day comparison is stated with its time basis: the
  // last day of the current scope is today's running total vs yesterday's FULL
  // day; every other pair is a full day vs its previous full day. There is no
  // hourly data, so no same-period comparison is claimed.
  const deltaBasisFor = (index: number): DeltaBasis =>
    historyIsCurrent && index === historyDays.length - 1
      ? "yesterday-full-day"
      : "previous-day";

  // Header day-over-day delta (today vs the previous day in the window).
  const prevIndex = summary.last7Days.length - 2;
  const headerDelta =
    prevIndex >= 0
      ? formatDelta(
          summary.today.totalTokens,
          summary.last7Days[prevIndex].totalTokens,
          lang,
          isCurrentScope ? "yesterday-full-day" : "previous-day",
        )
      : null;

  const hideTooltip = () => setActiveDay(null);

  return (
    <main className="shell">
      {/* Product, last-success time and Refresh live together in the sticky
          header so the data's age is always visible — never scrolled away with
          the footer. Freshness failures additionally surface in the banners
          directly below. */}
      <header className="app-header">
        <div>
          <p className="eyebrow">Coding Agent Monitor</p>
          <h1>{isCurrentScope ? d.today : d.usageFor(snapshot.scope.endDate)}</h1>
          <p className="header-updated">
            <time dateTime={summary.collectedAt}>
              {new Date(summary.collectedAt).toLocaleString(
                lang === "zh-CN" ? "zh-CN" : "en-US",
              )}
            </time>
            {" · "}
            {d.footerUpdated(updatedText)}
          </p>
        </div>
        <button
          className={collection.refreshing ? "refresh-btn refreshing" : "refresh-btn"}
          type="button"
          onClick={manualRefresh}
          disabled={collection.refreshing}
        >
          <svg
            className="refresh-icon"
            viewBox="0 0 16 16"
            aria-hidden="true"
            focusable="false"
          >
            <title />
            <path
              d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.7V6H10"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          {collection.refreshing ? d.refreshing : d.refresh}
        </button>
      </header>

      {closeNotice}

      {(refreshFailed || stale) && (
        <div className="stale-banner" role="status">
          <span>
            {refreshFailed && stale
              ? d.staleFailedOld(snapshot.scope.endDate)
              : refreshFailed
                ? d.staleFailedRecent
                : d.staleOld(snapshot.scope.endDate)}
          </span>
          <button type="button" onClick={manualRefresh}>
            {d.retry}
          </button>
        </div>
      )}

      {/* Success with skipped records: accepted totals stay, but coverage is
          flagged as a risk. Sanitized kinds + agent names only (no paths). */}
      {coverageText(summary.coverage, lang) !== "" && (
        <div className="stale-banner coverage-banner" role="status">
          <span>{coverageText(summary.coverage, lang)}</span>
        </div>
      )}

      <div className="dash-grid">
        <section className="dash-left" aria-labelledby="today-heading">
          <h2 id="today-heading" className="sr-only">
            {isCurrentScope ? d.todaySrHeading : d.todaySrHeadingFor(snapshot.scope.endDate)}
          </h2>
          <p className="total">{formatTokens(summary.today.totalTokens)}</p>
          {headerDelta?.label && (
            <p className={`total-delta ${deltaClass(headerDelta.kind)}`}>
              {headerDelta.label}
            </p>
          )}
          <p className="unit">{d.tokensUnit}</p>

          <p className="meta">
            <span
              className={
                cost.kind === "value" ? "meta-cost" : "meta-cost meta-cost-unknown"
              }
            >
              {cost.text}
            </span>
            {shareText !== null && (
              <>
                <span className="meta-sep" aria-hidden="true">
                  {" "}·{" "}
                </span>
                <span className="meta-cache">{d.cachedInput(shareText)}</span>
              </>
            )}
          </p>

          {todayAgents.length > 0 ? (
            <div className="agent-list">
              {todayAgents.map((agent) => {
                const meta = agentMeta(agent.id);
                const hasModels = agent.models.length > 0;
                const isOpen = expanded.has(agent.id);
                const modelCoverageGap = hasModels
                  ? Math.max(
                      0,
                      agent.tokens -
                        agent.models.reduce((sum, m) => sum + m.totalTokens, 0),
                    )
                  : 0;
                // Cached-input share summed across the agent's models, weighted by
                // tokens (never an average of per-model percentages). Shown only
                // for multi-model agents with no unattributed residual: the models
                // cover the denominator (input+cacheRead+cacheCreation) exactly,
                // so the aggregate is trustworthy. Any residual would make the
                // subset misrepresent the agent, so we hide it.
                const agentCacheShare =
                  hasModels &&
                  agent.models.length >= 2 &&
                  modelCoverageGap === 0
                    ? cacheInputShare(
                        agent.models.reduce((sum, m) => sum + m.inputTokens, 0),
                        agent.models.reduce(
                          (sum, m) => sum + m.cacheReadTokens,
                          0,
                        ),
                        agent.models.reduce(
                          (sum, m) => sum + m.cacheCreationTokens,
                          0,
                        ),
                      )
                    : null;
                // Agents with models: the WHOLE row (name + caret + tokens) is
                // one accessible toggle — a full-width click/keyboard target
                // with aria-expanded and a clear focus ring. The caret is
                // decorative; direction comes from aria-expanded, not colour.
                const rowContent = (
                  <>
                    <span className="agent-lead">
                      <AgentMark
                        colorVar={meta.colorVar}
                        glyph={agentMark(agent.displayName)}
                      />
                      <span className="agent-toggle-group">
                        {/* Agents without models keep an empty, faded spacer
                            so names align; it is not an expander. */}
                        {hasModels ? (
                          <span className="chevron" aria-hidden="true">
                            {isOpen ? "▾" : "▸"}
                          </span>
                        ) : (
                          <span className="chevron chevron-placeholder" aria-hidden="true" />
                        )}
                        <span className="agent-name">{agent.displayName}</span>
                      </span>
                    </span>
                    <span className="agent-tokens">
                      {formatTokens(agent.tokens)}
                    </span>
                  </>
                );
                return (
                  <div className="agent-block" key={agent.id}>
                    {hasModels ? (
                      <button
                        type="button"
                        className="agent-row agent-toggle"
                        onClick={() => toggleAgent(agent.id)}
                        aria-expanded={isOpen}
                        aria-controls={`agent-models-${agent.id}`}
                      >
                        {rowContent}
                      </button>
                    ) : (
                      <div className="agent-row">
                        {rowContent}
                      </div>
                    )}
                    {isOpen && hasModels && (
                      <div
                        className="agent-models"
                        id={`agent-models-${agent.id}`}
                      >
                        {(agent.reasoningTokens > 0 ||
                          agent.unclassifiedTokens > 0 ||
                          agentCacheShare !== null) && (
                          <div className="model-agent-notes">
                            {agent.reasoningTokens > 0 && (
                              <p className="model-agent-summary">
                                {d.includesReasoning(formatTokens(agent.reasoningTokens))}
                              </p>
                            )}
                            {agent.unclassifiedTokens > 0 && (
                              <p className="model-agent-summary">
                                {d.includesUnclassified(formatTokens(agent.unclassifiedTokens))}
                              </p>
                            )}
                            {agentCacheShare !== null && (
                              <p className="model-agent-summary">
                                {d.cacheAcrossModels(
                                  String(formatPercent(agentCacheShare)),
                                  agent.models.length,
                                )}
                              </p>
                            )}
                          </div>
                        )}
                        <dl className="model-list">
                          {agent.models.map((model) => {
                            const modelShare = cacheInputShare(
                              model.inputTokens,
                              model.cacheReadTokens,
                              model.cacheCreationTokens,
                            );
                            const composition = [
                              `${formatTokens(model.inputTokens)} ${d.modelComp.in}`,
                              `${formatTokens(model.outputTokens)} ${d.modelComp.out}`,
                              `${formatTokens(model.cacheReadTokens)} ${d.modelComp.cacheRead}`,
                              ...(model.cacheCreationTokens > 0
                                ? [`${formatTokens(model.cacheCreationTokens)} ${d.modelComp.creation}`]
                                : []),
                              ...(modelShare !== null
                                ? [d.cachedInput(String(formatPercent(modelShare)))]
                                : []),
                            ].join(" · ");
                            return (
                              <div className="model-row" key={model.modelName}>
                                <dt>
                                  <span className="model-name">
                                    {model.modelDisplayName}
                                  </span>
                                  <span className="model-composition">
                                    {composition}
                                  </span>
                                </dt>
                                <dd>{formatTokens(model.totalTokens)}</dd>
                              </div>
                            );
                          })}
                        </dl>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="empty-state">
              <p>
                {isCurrentScope ? d.emptyToday : d.emptyFor(snapshot.scope.endDate)}
              </p>
              <details className="empty-help">
                <summary>{d.whyNoUsage}</summary>
                <p>{d.emptyHelp}</p>
              </details>
            </div>
          )}

        </section>

        <section className="dash-right" aria-labelledby="trend-heading">
          <div className="section-heading">
            <h2 id="trend-heading">{historyRange === 7 ? d.last7Days : d.last30Days}</h2>
            <span>{historyRange === 30 && !history ? "—" : d.total(trendTotal <= BigInt(Number.MAX_SAFE_INTEGER) ? formatTokens(Number(trendTotal)) : trendTotal.toLocaleString(lang))}</span>
          <div className="history-controls" role="group" aria-label={d.historyRange}>
            <button type="button" aria-pressed={historyRange === 7} onClick={() => chooseRange(7)}>{d.days7}</button>
            <button type="button" aria-pressed={historyRange === 30} onClick={() => chooseRange(30)}>{d.days30}</button>
            {selectedDay && <button type="button" onClick={() => { setSelectedDay(null); setActiveDay(null); }}>{d.backToday}</button>}
          </div>
          </div>
          {historyRange === 30 && <div className="history-status" role="status">
            {historyStatus === "loading" ? d.historyLoading : historyStatus === "failed" ? <>{d.historyFailed} <button onClick={() => chooseRange(30)}>{d.retry}</button></> : history && <>
              <p>{history.scope.startDate} – {history.scope.endDate} · {history.scope.timeZone} · {d.historyIncludesToday}</p>
              <p>{relativeTime(history.collectedAt, new Date(), lang)} · {costDisplay(history.estimatedCostUsd, null, history.days.some(day => day.totalTokens > 0), lang).text}</p>
              {(!historyIsCurrent || historyExpired) && <p>{d.historyOld}</p>}
              <p>{coverageText(history.coverage, lang)}</p>
            </>}
          </div>}
          {/* Trend filter chips: capped inline; the overflow stays keyboard-
              reachable behind a native details disclosure. The chip state and
              the swap rule follow the EFFECTIVE filter, so a selection whose
              agent vanished renders as All instead of a stuck pressed chip. */}
          <div className="trend-filter" role="group" aria-label={d.filterByAgent}>
            <button
              type="button"
              className={activeFilter === null ? "filter-chip active" : "filter-chip"}
              aria-pressed={activeFilter === null}
              onClick={() => setAgentFilter(null)}
            >
              {d.all}
            </button>
            {chipLayout.visible.map((agent) => {
              const meta = agentMeta(agent.id);
              const active = activeFilter === agent.id;
              return (
                <button
                  type="button"
                  key={agent.id}
                  className={
                    active ? "filter-chip agent-chip active" : "filter-chip agent-chip"
                  }
                  aria-pressed={active}
                  onClick={() => setAgentFilter(active ? null : agent.id)}
                  style={
                    {
                      "--chip-color": `var(${meta.colorVar})`,
                      "--chip-soft": `var(${meta.softVar})`,
                    } as CSSProperties
                  }
                >
                  <AgentMark colorVar={meta.colorVar} glyph={agentMark(agent.displayName)} />
                  {agent.displayName}
                </button>
              );
            })}
            {chipLayout.hidden.length > 0 && (
              <details
                className="filter-more"
                ref={moreMenuRef}
                onKeyDown={(event) => {
                  if (event.key === "Escape" && moreMenuRef.current?.open) {
                    moreMenuRef.current.open = false;
                    moreMenuSummaryRef.current?.focus();
                  }
                }}
              >
                <summary
                  className="filter-chip filter-more-summary"
                  ref={moreMenuSummaryRef}
                >
                  <AgentMark colorVar="--agent-others" glyph="+" />
                  {d.moreAgents(chipLayout.hidden.length)}
                </summary>
                <div
                  className="filter-more-menu"
                  role="group"
                  aria-label={d.moreAgents(chipLayout.hidden.length)}
                >
                  {chipLayout.hidden.map((agent) => {
                    const meta = agentMeta(agent.id);
                    const active = activeFilter === agent.id;
                    return (
                      <button
                        type="button"
                        key={agent.id}
                        className={
                          active ? "filter-chip agent-chip active" : "filter-chip agent-chip"
                        }
                        aria-pressed={active}
                        onClick={() => {
                          setAgentFilter(active ? null : agent.id);
                          if (moreMenuRef.current) moreMenuRef.current.open = false;
                        }}
                        style={
                          {
                            "--chip-color": `var(${meta.colorVar})`,
                            "--chip-soft": `var(${meta.softVar})`,
                          } as CSSProperties
                        }
                      >
                        <AgentMark colorVar={meta.colorVar} glyph={agentMark(agent.displayName)} />
                        {agent.displayName}
                      </button>
                    );
                  })}
                </div>
              </details>
            )}
          </div>
          {/* Scope note: the filter never touches today's summary above. */}
          <p className="filter-note">{d.filterTrendOnly}</p>

          <div className="history-chart-scroll">
          <div className={historyRange === 30 ? "trend trend-30" : "trend"}>
            {chartDays.map((chartDay, index) => {
              const day = historyDays[index];
              const isSelected = selectedDay === day.date;
              const valueLabel = formatTokens(trendSeries[index]);
              const aria = activeFilter === null
                ? allDayAriaLabel(
                    day,
                    index > 0
                      ? historyDays[index - 1].totalTokens
                      : undefined,
                    lang,
                    deltaBasisFor(index),
                  )
                : agentDayAriaLabel(
                    day,
                    activeFilter,
                    index > 0
                      ? (historyDays[index - 1].agents.find(
                          (a) => a.id === activeFilter,
                        )?.tokens ?? 0)
                      : undefined,
                    lang,
                    deltaBasisFor(index),
                  );
              return (
                <button
                  type="button"
                  className={isSelected ? "trend-day selected" : "trend-day"}
                  key={day.date}
                  ref={(el) => {
                    dayEls.current[index] = el;
                  }}
                  aria-label={aria}
                  aria-pressed={isSelected}
                  onMouseEnter={() => setActiveDay(index)}
                  onMouseLeave={hideTooltip}
                  onFocus={() => setActiveDay(index)}
                  onBlur={hideTooltip}
                  // Click pins the day into the stable in-flow detail panel;
                  // clicking the selected day again unpins it. Escape clears
                  // both the hover tooltip and the pinned selection.
                  onClick={() => setSelectedDay(isSelected ? null : day.date)}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") {
                      hideTooltip();
                      setSelectedDay(null);
                    }
                  }}
                >
                  <span className="trend-value">{valueLabel}</span>
                  <span className="bar-track">
                    {chartDay.segments.map((seg) => (
                      <span
                        key={seg.agentId}
                        className={`bar-segment${seg.isTop ? " is-top" : ""}`}
                        style={{
                          height: `${seg.height}%`,
                          background: `var(${seg.colorVar})`,
                        }}
                      />
                    ))}
                    <span className="bar-baseline" aria-hidden="true" />
                  </span>
                  <span className="trend-date">{shortDate(day.date)}</span>
                </button>
              );
            })}
          </div>

          </div>
          {/* Scale semantics: the All chart and a single-agent chart each rescale
              to their own window max, so the same bar height never implies the
              same amount across filters; the per-bar values stay absolute. */}
          <p className="scale-hint">
            {activeFilter === null
              ? d.scaleAll
              : d.scaleAgent(activeFilterName ?? "")}
          </p>

          {/* Hover/focus tooltip — the fixed, clamped overlay. It is suppressed
              on the pinned day, whose details already live in the panel below. */}
          {activeDay !== null &&
            historyDays[activeDay] && historyDays[activeDay]?.date !== selectedDay && (
              <div
                className="chart-tooltip"
                role="tooltip"
                ref={tooltipEl}
                style={tooltipPos ?? undefined}
              >
                <DayDetailContent
                  day={historyDays[activeDay]!}
                  prevDay={
                    activeDay > 0 ? historyDays[activeDay - 1] : undefined
                  }
                  filter={activeFilter}
                  filterName={activeFilterName}
                  grouping={allChart.grouping}
                  d={d}
                  lang={lang}
                  basis={deltaBasisFor(activeDay)}
                />
              </div>
            )}

          {/* Click-pinned stable detail: rendered from the selected ISO date, so
              refreshes and window shifts can never misalign it — a date that
              leaves the visible range simply closes the panel. */}
          {(() => {
            const selected =
              selectedDay !== null
                ? historyDays.find((day) => day.date === selectedDay)
                : undefined;
            if (!selected) return null;
            const selectedIndex = historyDays.indexOf(selected);
            return (
              <div
                className="day-detail"
                role="region"
                aria-label={d.dayDetailRegion}
              >
                <DayDetailContent
                  day={selected}
                  prevDay={
                    selectedIndex > 0
                      ? historyDays[selectedIndex - 1]
                      : undefined
                  }
                  filter={activeFilter}
                  filterName={activeFilterName}
                  grouping={allChart.grouping}
                  d={d}
                  lang={lang}
                  basis={deltaBasisFor(selectedIndex)}
                />
                <p>{costDisplay(selected.estimatedCostUsd, selected.costUnknownReason, selected.totalTokens > 0, lang).text}</p>
                <p>{coverageText(historyRange === 30 && history ? history.coverage : summary.coverage, lang)}</p>
                <p>{d.selectedDayAll}</p>
                <BreakdownList total={selected.totalTokens} breakdown={selected.tokenBreakdown} d={d} />
                {sortAgents(selected.agents).map(agent => <details key={agent.id} className="history-models">
                  <summary>{agent.displayName} · {agent.tokens.toLocaleString(lang)} Token</summary>
                  {agent.reasoningTokens > 0 && <p>{d.includesReasoning(agent.reasoningTokens.toLocaleString(lang))}</p>}
                  {agent.unclassifiedTokens > 0 && <p>{d.includesUnclassified(agent.unclassifiedTokens.toLocaleString(lang))}</p>}
                  {agent.models.map(model => <div key={model.modelName}>
                    <p>{model.modelDisplayName} · {model.totalTokens.toLocaleString(lang)} Token</p>
                    <BreakdownList total={model.totalTokens} breakdown={{ ...model, reasoningTokens: 0, unclassifiedTokens: 0 }} d={d} />
                  </div>)}
                  {agent.models.length === 0 && <p>{d.noModelDetails}</p>}
                </details>)}

              </div>
            );
          })()}
        </section>
      </div>

      {/* On-demand detail tier: today's token composition and the statistics
          explainer expand here, below the core summary and the trend. */}
      <section className="dash-details">
        <details className="breakdown-section">
          <summary className="section-heading breakdown-summary">
            <h2 id="breakdown-heading">{d.tokenBreakdown}</h2>
            <span>
              {isCurrentScope
                ? d.shareOfToday
                : d.shareOf(snapshot.scope.endDate)}
            </span>
          </summary>
          <BreakdownList
            total={summary.today.totalTokens}
            breakdown={summary.today.tokenBreakdown}
            d={d}
          />
        </details>

        {/* Statistics explainer: what is counted, how token types are
            defined, the cached-input denominator, and the cost estimate's
            meaning. Static, local-only copy — no data dependency. */}
        <details className="about-stats">
          <summary>{d.aboutTitle}</summary>
          <div className="about-body">
            <p>
              <strong>{d.aboutScope}</strong> {d.aboutScopeBody}
            </p>
            <p>
              <strong>{d.aboutTypes}</strong> {d.aboutTypesBody}
            </p>
            <p>
              <strong>{d.aboutCache}</strong>
              {d.aboutCacheBody}
            </p>
            <p>
              <strong>{d.aboutCost}</strong>
              {d.aboutCostBody}
            </p>
          </div>
        </details>

        {/* Minimal preferences (T05): launch behaviour and language only —
            a collapsed section inside the existing window, no settings
            centre. Hidden entirely when preferences are unavailable. */}
        {prefs && (
          <details className="settings-section">
            <summary>{d.settingsTitle}</summary>
            <div className="settings-body">
              <label className="setting-row">
                <span className="setting-name">{d.languageLabel}</span>
                <select
                  value={prefs.language}
                  onChange={(event) =>
                    changeLanguage(event.target.value as LanguagePreference)
                  }
                >
                  <option value="system">{d.languageSystem}</option>
                  <option value="zh-CN">中文（简体）</option>
                  <option value="en">English</option>
                </select>
              </label>
              <label className="setting-row setting-check">
                <input
                  type="checkbox"
                  checked={prefs.startWithWindows}
                  onChange={(event) =>
                    void changePrefs({
                      startWithWindows: event.target.checked,
                    })
                  }
                />
                <span>{d.startWithWindows}</span>
              </label>
              <label className="setting-row setting-check">
                <input
                  type="checkbox"
                  checked={prefs.startHiddenToTray}
                  onChange={(event) =>
                    void changePrefs({
                      startHiddenToTray: event.target.checked,
                    })
                  }
                />
                <span>{d.startHiddenToTray}</span>
              </label>
              {settingsError && (
                <p className="settings-error" role="status">
                  {d.settingsSaveFailed}
                </p>
              )}
            </div>
          </details>
        )}
      </section>
    </main>
  );
}

export default App;
