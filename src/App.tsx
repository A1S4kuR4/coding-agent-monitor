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
import { fetchUsageSummary } from "./lib/usage-api";
import type { TokenBreakdown, UsageSummary } from "./types/usage";
import { formatTokens } from "./features/usage/formatTokens";
import { activeAgentRows } from "./features/usage/usageRows";
import { viewReducer } from "./features/usage/viewState";
import { formatUsd } from "./features/usage/formatUsd";
import { formatPercent } from "./features/usage/formatPercent";
import { cacheInputShare } from "./features/usage/cacheInputShare";
import { relativeTime } from "./features/usage/relativeTime";
import { agentMeta, compareByMeta, sortAgents } from "./features/usage/agents";
import { formatDelta } from "./features/usage/formatDelta";
import { buildAllChart, buildAgentChart, dayValue } from "./features/usage/chartView";
import { allDayAriaLabel, agentDayAriaLabel, fullDate } from "./features/usage/tooltipLabel";

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return "Unable to load usage data.";
}

function shortDate(date: string): string {
  return date.slice(5).replace("-", "/");
}

const deltaClass = (kind: string) => `delta ${kind}`;

/** Today's token composition by type, each with its count and share of the day.
 * Input, Output and Cache read always render; optional known types and the
 * explicit unclassified fallback render only when they carry tokens. */
function BreakdownList({
  total,
  breakdown,
}: {
  total: number;
  breakdown: TokenBreakdown;
}) {
  const parts: { key: string; label: string; value: number }[] = [
    { key: "input", label: "Input", value: breakdown.inputTokens },
    { key: "output", label: "Output", value: breakdown.outputTokens },
    { key: "cacheRead", label: "Cache read", value: breakdown.cacheReadTokens },
  ];
  if (breakdown.cacheCreationTokens > 0) {
    parts.push({
      key: "cacheCreation",
      label: "Cache creation",
      value: breakdown.cacheCreationTokens,
    });
  }
  if (breakdown.reasoningTokens > 0) {
    parts.push({
      key: "reasoning",
      label: "Reasoning",
      value: breakdown.reasoningTokens,
    });
  }
  if (breakdown.unclassifiedTokens > 0) {
    parts.push({
      key: "unclassified",
      label: "Unclassified",
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
              <span className="breakdown-count">
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

function App() {
  // The view machine lives in a pure reducer so its transitions are unit-tested
  // and no effect ever calls setState synchronously.
  const [view, dispatch] = useReducer(viewReducer, { status: "loading" });
  // Bumps every minute to refresh the relative-time label (see the interval
  // effect); the value itself is never read.
  const [, setTick] = useState(0);
  // Which agent ids currently have their per-model breakdown expanded.
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  // The seven-day trend filter: `null` means All agents, otherwise an agent id.
  const [agentFilter, setAgentFilter] = useState<string | null>(null);
  // Toolkit for the per-day bar: which day (index) is active, and its computed
  // fixed-position placement. `null` hides the tooltip.
  const [activeDay, setActiveDay] = useState<number | null>(null);
  const [tooltipPos, setTooltipPos] = useState<{ left: number; top: number } | null>(
    null,
  );
  const dayEls = useRef<Array<HTMLElement | null>>([]);
  const tooltipEl = useRef<HTMLDivElement | null>(null);
  // Holds the async-returned unlisten functions so cleanup can release them even
  // when registration resolved after an unmount.
  const focusFetchInFlight = useRef(false);
  const focusUnlisten = useRef<(() => void) | undefined>(undefined);
  const trayUnlisten = useRef<(() => void) | undefined>(undefined);

  // Initial load. State changes only inside the promise callbacks, never in the
  // effect body itself.
  useEffect(() => {
    let active = true;
    void fetchUsageSummary().then(
      (summary) => {
        if (active) dispatch({ type: "load-succeeded", summary });
      },
      (error: unknown) => {
        if (active)
          dispatch({
            type: "load-failed",
            keepExisting: false,
            message: errorMessage(error),
          });
      },
    );
    return () => {
      active = false;
    };
  }, []);

  // Refetch when the window regains focus (it is never remounted when hidden to
  // the tray, so a mount-time fetch would otherwise show stale numbers). The
  // listener registration is async: cleanup handles both an already-resolved
  // unlisten and one that lands after unmount.
  useEffect(() => {
    let active = true;
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (!active || !focused || focusFetchInFlight.current) return;
        focusFetchInFlight.current = true;
        dispatch({ type: "refresh-started" });
        void fetchUsageSummary()
          .then(
            (summary) => {
              if (active) dispatch({ type: "load-succeeded", summary });
            },
            (error: unknown) => {
              if (active)
                dispatch({
                  type: "load-failed",
                  keepExisting: true,
                  message: errorMessage(error),
                });
            },
          )
          .finally(() => {
            focusFetchInFlight.current = false;
          });
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
  }, []);

  // The tray's periodic refresh emits the same snapshot to any open window.
  // Apply it directly — never start a competing fetch (there is no sidecar).
  useEffect(() => {
    let active = true;
    void listen<UsageSummary>("usage-updated", (event) => {
      if (active) dispatch({ type: "event-received", summary: event.payload });
    }).then(
      (fn) => {
        if (active) {
          trayUnlisten.current = fn;
        } else {
          fn();
        }
      },
      () => undefined,
    );
    return () => {
      active = false;
      trayUnlisten.current?.();
      trayUnlisten.current = undefined;
    };
  }, []);

  // Refresh only the relative-time wording on a timer; this never hits the
  // sidecar, so the label can tick without extra collection.
  useEffect(() => {
    const id = window.setInterval(() => setTick((value) => value + 1), 60_000);
    return () => window.clearInterval(id);
  }, [setTick]);

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
  }, [positionTooltip, agentFilter]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const retry = () => {
    dispatch({ type: "load-started" });
    void fetchUsageSummary().then(
      (summary) => dispatch({ type: "load-succeeded", summary }),
      (error: unknown) =>
        dispatch({
          type: "load-failed",
          keepExisting: false,
          message: errorMessage(error),
        }),
    );
  };

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
    dispatch({ type: "refresh-started" });
    void fetchUsageSummary().then(
      (summary) => dispatch({ type: "load-succeeded", summary }),
      (error: unknown) =>
        dispatch({
          type: "load-failed",
          keepExisting: true,
          message: errorMessage(error),
        }),
    );
  };

  if (view.status === "loading") {
    return (
      <main className="shell status-panel" aria-live="polite">
        <p className="eyebrow">Coding Agent Monitor</p>
        <h1>Loading usage…</h1>
      </main>
    );
  }

  if (view.status === "error") {
    return (
      <main className="shell status-panel" role="alert">
        <p className="eyebrow">Coding Agent Monitor</p>
        <h1>Usage unavailable</h1>
        <p className="status-copy">{view.message}</p>
        <button type="button" onClick={retry}>
          Try again
        </button>
      </main>
    );
  }

  const { summary } = view;
  const todayAgents = activeAgentRows(summary.today.agents);
  const costText = formatUsd(summary.today.estimatedCostUsd);
  const shareText = formatPercent(summary.today.cacheReadShare);
  const updatedText = relativeTime(summary.collectedAt, new Date());

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
  for (const day of summary.last7Days) {
    for (const agent of day.agents) {
      if (!seen.has(agent.id)) {
        seen.add(agent.id);
        recognized.push({ id: agent.id, displayName: agent.displayName });
      }
    }
  }
  recognized.sort(compareByMeta);

  // The seven-day chart honours the agent filter. `chartDays` is the pure
  // view-model (stacked vs single-agent); `trendSeries` feeds the per-day axis
  // labels and the "Total" aggregate.
  const chartDays =
    agentFilter === null
      ? buildAllChart(summary.last7Days)
      : buildAgentChart(summary.last7Days, agentFilter);
  const trendSeries = summary.last7Days.map((d) => dayValue(d, agentFilter));
  const trendTotal = trendSeries.reduce((sum, value) => sum + value, 0);

  // Header day-over-day delta (today vs the previous day in the window).
  const prevIndex = summary.last7Days.length - 2;
  const headerDelta =
    prevIndex >= 0
      ? formatDelta(
          summary.today.totalTokens,
          summary.last7Days[prevIndex].totalTokens,
        )
      : null;

  const hideTooltip = () => setActiveDay(null);

  return (
    <main className="shell">
      <header className="app-header">
        <div>
          <p className="eyebrow">Coding Agent Monitor</p>
          <h1>Today</h1>
        </div>
        <button
          className={view.refreshing ? "refresh-btn refreshing" : "refresh-btn"}
          type="button"
          onClick={manualRefresh}
          disabled={view.refreshing}
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
          {view.refreshing ? "Refreshing…" : "Refresh"}
        </button>
      </header>

      {view.stale && (
        <div className="stale-banner" role="status">
          <span>Couldn&apos;t refresh — showing last known data.</span>
          <button type="button" onClick={manualRefresh}>
            Retry
          </button>
        </div>
      )}

      <div className="dash-grid">
        <section className="dash-left" aria-labelledby="today-heading">
          <h2 id="today-heading" className="sr-only">
            Today&apos;s token usage
          </h2>
          <p className="total">{formatTokens(summary.today.totalTokens)}</p>
          {headerDelta?.label && (
            <p className={`total-delta ${deltaClass(headerDelta.kind)}`}>
              {headerDelta.label}
            </p>
          )}
          <p className="unit">Tokens</p>

          {(costText !== null || shareText !== null) && (
            <p className="meta">
              {costText !== null && <span className="meta-cost">Est. cost {costText}</span>}
              {costText !== null && shareText !== null && (
                <span className="meta-sep" aria-hidden="true">
                  {" "}·{" "}
                </span>
              )}
              {shareText !== null && (
                <span className="meta-cache">~{shareText} cached input</span>
              )}
            </p>
          )}

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
                return (
                  <div className="agent-block" key={agent.id}>
                    <div className="agent-row">
                      <span className="agent-lead">
                        <span
                          className="agent-dot"
                          aria-hidden="true"
                          style={{ background: `var(${meta.colorVar})` }}
                        />
                        <span className="agent-toggle-group">
                          {hasModels ? (
                            <button
                              type="button"
                              className="agent-toggle"
                              onClick={() => toggleAgent(agent.id)}
                              aria-expanded={isOpen}
                              aria-controls={`agent-models-${agent.id}`}
                              aria-label={`Toggle ${agent.displayName} models`}
                            >
                              <span className="chevron" aria-hidden="true">
                                {isOpen ? "▾" : "▸"}
                              </span>
                            </button>
                          ) : (
                            <span
                              className="chevron chevron-placeholder"
                              aria-hidden="true"
                            />
                          )}
                          <span className="agent-name">{agent.displayName}</span>
                        </span>
                      </span>
                      <span className="agent-tokens">{formatTokens(agent.tokens)}</span>
                    </div>
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
                                Agent total includes{" "}
                                {formatTokens(agent.reasoningTokens)} reasoning
                              </p>
                            )}
                            {agent.unclassifiedTokens > 0 && (
                              <p className="model-agent-summary">
                                Agent total includes{" "}
                                {formatTokens(agent.unclassifiedTokens)} unclassified
                                tokens
                              </p>
                            )}
                            {agentCacheShare !== null && (
                              <p className="model-agent-summary">
                                ~{formatPercent(agentCacheShare)} cached input across{" "}
                                {agent.models.length} models
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
                            return (
                              <div className="model-row" key={model.modelName}>
                                <dt>
                                  <span className="model-name">
                                    {model.modelDisplayName}
                                  </span>
                                  <span className="model-composition">
                                    {formatTokens(model.inputTokens)} in ·{" "}
                                    {formatTokens(model.outputTokens)} out ·{" "}
                                    {formatTokens(model.cacheReadTokens)} cache read
                                    {model.cacheCreationTokens > 0
                                      ? ` · ${formatTokens(model.cacheCreationTokens)} creation`
                                      : ""}
                                    {modelShare !== null
                                      ? ` · ~${formatPercent(modelShare)} cached input`
                                      : ""}
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
            <p className="empty-state">No agent usage was found for today.</p>
          )}

          <section className="breakdown-section" aria-labelledby="breakdown-heading">
            <div className="section-heading">
              <h2 id="breakdown-heading">Token Breakdown</h2>
              <span>Share of today</span>
            </div>
            <BreakdownList
              total={summary.today.totalTokens}
              breakdown={summary.today.tokenBreakdown}
            />
          </section>
        </section>

        <section className="dash-right" aria-labelledby="trend-heading">
          <div className="section-heading">
            <h2 id="trend-heading">Last 7 Days</h2>
            <span>Total {formatTokens(trendTotal)}</span>
          </div>

          <div className="trend-filter" role="group" aria-label="Filter by agent">
            <button
              type="button"
              className={agentFilter === null ? "filter-chip active" : "filter-chip"}
              aria-pressed={agentFilter === null}
              onClick={() => setAgentFilter(null)}
            >
              All
            </button>
            {recognized.map((agent) => {
              const meta = agentMeta(agent.id);
              const active = agentFilter === agent.id;
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
                  <span className="chip-dot" aria-hidden="true" />
                  {agent.displayName}
                </button>
              );
            })}
          </div>

          <div className="trend">
            {chartDays.map((chartDay, index) => {
              const day = summary.last7Days[index];
              const valueLabel = formatTokens(trendSeries[index]);
              const aria = agentFilter === null
                ? allDayAriaLabel(
                    day,
                    index > 0
                      ? summary.last7Days[index - 1].totalTokens
                      : undefined,
                  )
                : agentDayAriaLabel(
                    day,
                    agentFilter,
                    index > 0
                      ? (summary.last7Days[index - 1].agents.find(
                          (a) => a.id === agentFilter,
                        )?.tokens ?? 0)
                      : undefined,
                  );
              return (
                <button
                  type="button"
                  className="trend-day"
                  key={day.date}
                  ref={(el) => {
                    dayEls.current[index] = el;
                  }}
                  aria-label={aria}
                  onMouseEnter={() => setActiveDay(index)}
                  onMouseLeave={hideTooltip}
                  onFocus={() => setActiveDay(index)}
                  onBlur={hideTooltip}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") hideTooltip();
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

          {activeDay !== null && (
            (() => {
              const day = summary.last7Days[activeDay];
              const isAll = agentFilter === null;
              const prevTotal = activeDay > 0 ? summary.last7Days[activeDay - 1].totalTokens : undefined;
              const allDelta = formatDelta(day.totalTokens, prevTotal);
              const sel = agentFilter ? day.agents.find((a) => a.id === agentFilter) : null;
              const selValue = sel?.tokens ?? 0;
              const selPrev = agentFilter && activeDay > 0
                ? (summary.last7Days[activeDay - 1].agents.find(
                    (a) => a.id === agentFilter,
                  )?.tokens ?? 0)
                : undefined;
              const selDelta = agentFilter ? formatDelta(selValue, selPrev) : null;
              const selShare = day.totalTokens > 0 ? (selValue / day.totalTokens) * 100 : 0;
              const selName = agentFilter
                ? (sel?.displayName ?? agentMeta(agentFilter).displayName)
                : "";
              return (
                <div
                  className="chart-tooltip"
                  role="tooltip"
                  ref={tooltipEl}
                  style={tooltipPos ?? undefined}
                >
                  {isAll ? (
                    <>
                      <p className="tooltip-date">{fullDate(day.date)}</p>
                      <p className="tooltip-total">
                        {formatTokens(day.totalTokens)} tokens total
                      </p>
                      <ul className="tooltip-agents">
                        {sortAgents(day.agents).map((a) => {
                          const pct =
                            day.totalTokens > 0
                              ? (a.tokens / day.totalTokens) * 100
                              : 0;
                          return (
                            <li key={a.id}>
                              <span
                                className="tooltip-dot"
                                aria-hidden="true"
                                style={{
                                  background: `var(${agentMeta(a.id).colorVar})`,
                                }}
                              />
                              <span className="tooltip-agent-name">{a.displayName}</span>
                              <span className="tooltip-agent-value">
                                {formatTokens(a.tokens)} · {pct.toFixed(1)}%
                              </span>
                            </li>
                          );
                        })}
                      </ul>
                      {allDelta.label && (
                        <p className={`tooltip-delta ${deltaClass(allDelta.kind)}`}>
                          {allDelta.label}
                        </p>
                      )}
                    </>
                  ) : (
                    <>
                      <p className="tooltip-date">{fullDate(day.date)}</p>
                      <p className="tooltip-total">
                        {selName}: {formatTokens(selValue)}
                      </p>
                      <p className="tooltip-share">{selShare.toFixed(1)}% of day</p>
                      {selDelta?.label && (
                        <p className={`tooltip-delta ${deltaClass(selDelta.kind)}`}>
                          {selDelta.label}
                        </p>
                      )}
                    </>
                  )}
                </div>
              );
            })()
          )}
        </section>
      </div>

      <footer className="app-footer">
        <p className="updated">Updated {updatedText}</p>
      </footer>
    </main>
  );
}

export default App;
