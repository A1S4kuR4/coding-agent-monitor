/**
 * Lightweight bilingual dictionary + shared language resolution (T03).
 *
 * The supported set is Simplified Chinese and English (v0.4 plan §4.5). The
 * default preference is `system`: the language is resolved once per render
 * surface at startup — `navigator.language` for the window, the Windows UI
 * language for the tray (see `src-tauri/src/lang.rs`) — and unsupported system
 * languages fall back to English. T05 adds an explicit persisted choice
 * (`system`/`zh-CN`/`en`, see `src-tauri/src/prefs.rs`); an explicit choice
 * overrides the system resolution on both surfaces and stays one language for
 * the whole session. Agent names, open agent ids and model ids are dynamic
 * user data and never enter these dictionaries; Token/USD units and exact
 * numeric semantics are identical in both languages.
 */
export type Language = "zh-CN" | "en";

/** Maps a BCP-47-ish tag to the supported set: any `zh*` tag is Simplified
 * Chinese; every other (or missing) value falls back to English. */
export function resolveLanguage(tag: string | null | undefined): Language {
  if (tag && tag.trim().toLowerCase().startsWith("zh")) return "zh-CN";
  return "en";
}

/** Resolves the webview's system language. Never throws; an unavailable
 * navigator falls back to English. */
export function systemLanguage(): Language {
  try {
    const nav = navigator as Navigator;
    return resolveLanguage(nav.languages?.[0] ?? nav.language);
  } catch {
    return "en";
  }
}

const en = {
  last30Days: "Last 30 days",
  historyRange: "History range",
  days7: "7 days",
  days30: "30 days",
  backToday: "Back to today",
  historyLoading: "Loading 30-day history…",
  historyFailed: "History could not be loaded.",
  historyIncludesToday: "Includes query date; local calendar days",
  historyOld: "Historical snapshot — expired or date/time zone changed. Refresh to update.",
  selectedDayAll: "Selected date · token and model details (all agents)",
  noModelDetails: "No model breakdown available.",
  loadingTitle: "Loading usage…",
  errorTitle: "Usage unavailable",
  errorTimedOut: "The usage refresh did not finish in time. You can try again.",
  errorCancelled: "The usage refresh was cancelled. You can try again.",
  errorFailed: "Usage could not be refreshed. You can try again.",
  errorTransport: "Unable to read the local usage state. You can try again.",
  errorHint:
    "Collection reads local coding-agent records on this machine. If it keeps failing, check that a supported coding agent has been used here.",
  tryAgain: "Try again",
  retry: "Retry",
  refresh: "Refresh",
  refreshing: "Refreshing…",
  today: "Today",
  usageFor: (date: string) => `Usage for ${date}`,
  staleFailedOld: (date: string) =>
    `Refresh failed — showing out-of-date data for ${date}.`,
  staleFailedRecent: "Refresh failed — the data below is still recent.",
  staleOld: (date: string) => `Showing out-of-date data for ${date}.`,
  todaySrHeading: "Today’s token usage",
  todaySrHeadingFor: (date: string) => `Token usage for ${date}`,
  tokensUnit: "Tokens",
  // `pct` arrives pre-formatted by formatPercent (e.g. "66%").
  cachedInput: (pct: string) => `~${pct} cached input`,
  emptyToday: "No agent usage was found for today.",
  emptyFor: (date: string) => `No agent usage was found for ${date}.`,
  whyNoUsage: "Why no usage?",
  emptyHelp:
    "Counts only include usage recorded on this Windows user account during the dates shown. If you expected usage, check that a supported coding agent was actually used on this machine within the range.",
  tokenBreakdown: "Token Breakdown",
  shareOfToday: "Share of today",
  shareOf: (date: string) => `Share of ${date}`,
  breakdown: {
    input: "Input",
    output: "Output",
    cacheRead: "Cache read",
    cacheCreation: "Cache creation",
    reasoning: "Reasoning",
    unclassified: "Unclassified",
  },
  aboutTitle: "About these numbers",
  aboutScope: "Scope.",
  aboutScopeBody:
    "Counts come from coding-agent usage records stored on this Windows user account for the dates shown (your local time zone). Nothing leaves this machine.",
  aboutTypes: "Token types.",
  aboutTypesBody:
    "Input: prompt tokens sent to the model. Output: tokens the model generated. Cache read: prompt tokens served from the provider’s prompt cache. Cache creation: prompt tokens written to that cache. Reasoning: hidden thinking tokens that some agents report outside input/output. Unclassified: whatever the source reports beyond these types.",
  aboutCache: "Cached input share",
  aboutCacheBody:
    " = cache read ÷ (input + cache read + cache creation). Output tokens are not part of this denominator.",
  aboutCost: "Estimated cost",
  aboutCostBody:
    " is a reference estimate computed from token counts using the offline price table bundled with the app. It is not a bill, subscription charge, or account credit or limit. When a contributing model has no price, the cost shows as unavailable instead of a partial sum or a fake $0.00. When there is no usage, cost is N/A; $0.00 is reserved for usage with a complete zero-cost estimate.",
  last7Days: "Last 7 Days",
  total: (n: string) => `Total ${n}`,
  filterByAgent: "Filter by agent",
  filterTrendOnly:
    "Filter affects the trend only — today's summary above is always complete.",
  all: "All",
  moreAgents: (n: number) => `More agents (${n})`,
  otherAgents: (n: number) => `Other agents (${n})`,
  scaleAll:
    "Bar height scales to the busiest day of the window; filtering rescales the chart.",
  scaleAgent: (name: string) => `Bar height scales to ${name}’s busiest day.`,
  dayDetailRegion: "Selected day details",
  footerLastRefresh: "Last successful refresh",
  footerUpdated: (rel: string) => `Updated ${rel}`,
  justNow: "just now",
  minutesAgo: (m: number) => `${m}m ago`,
  hoursAgo: (h: number) => `${h}h ago`,
  daysAgo: (d: number) => `${d}d ago`,
  tooltipTokensTotal: (n: string) => `${n} tokens total`,
  tooltipOfDay: (pct: string) => `${pct}% of day`,
  includesReasoning: (n: string) => `Agent total includes ${n} reasoning`,
  includesUnclassified: (n: string) =>
    `Agent total includes ${n} unclassified tokens`,
  cacheAcrossModels: (pct: string, models: number) =>
    `~${pct} cached input across ${models} models`,
  modelComp: { in: "in", out: "out", cacheRead: "cache read", creation: "creation" },
  costValue: (usd: string) => `Est. cost ${usd}`,
  costUnavailable: "Est. cost unavailable",
  costMissingPrices: "Est. cost unavailable — missing model prices",
  costNoUsage: "Est. cost N/A — no usage",
  // Direction is expressed neutrally (arrow + sign only); colour never judges
  // good/bad. The header and today-tooltip comparisons are today's running
  // total vs yesterday's FULL day, so they carry the explicit full-day basis.
  deltaVsYesterdayFullDay: "vs yesterday (full day)",
  deltaVsPreviousDay: "vs previous day",
  deltaVsPreviousWeek: "vs previous week",
  deltaNoYesterday: "— no prior-day data",
  deltaNoUsageYesterday: "— yesterday had no usage",
  ariaTotal: (n: string) => `${n} tokens total.`,
  ariaAgentShare: (name: string, n: string, pct: string) =>
    `${name}: ${n} tokens (${pct}%).`,
  ariaAgentOfDay: (name: string, n: string, pct: string) =>
    `${name} ${n} tokens (${pct}% of day).`,
  // Minimal preferences section (T05): launch behaviour + language only — no
  // settings centre. The first-close notice explains tray residency once.
  settingsTitle: "Preferences",
  languageLabel: "Language",
  languageSystem: "System",
  startWithWindows: "Start with Windows",
  startHiddenToTray: "Hide to tray on startup",
  settingsSaveFailed: "This preference could not be saved.",
  // Today-section composition strip (review B1 + A3) and the pinned-day close
  // button (review C2).
  compDimensions: "Composition dimension",
  compByAgent: "By agent",
  compByType: "By type",
  compExpand: "Expand detail",
  compCollapse: "Collapse detail",
  compBarLabel: "Today's token composition — activate to toggle the detail rows",
  closeDetail: "Close details",
  // 30-day week aggregation (review B2) and the cost negative-state demotion
  // (review A2). The short cost mark keeps the full reason in the tooltip and
  // the statistics explainer; the three cost states never collapse into one.
  aggMode: "Aggregation",
  aggWeek: "By week",
  aggDay: "By day",
  scaleAllWeek:
    "Bar height scales to the busiest week of the window; filtering rescales the chart.",
  costNa: "Cost n/a ⓘ",
  selectedWeekAll: "Selected week · token and model details (all agents)",
  closeNoticeTitle: "Still running in the tray",
  closeNoticeBody:
    "Closing the window keeps Coding Agent Monitor running in the system tray. Reopen it from the tray icon, or exit from the tray menu.",
  closeNoticeAcknowledge: "Got it — don’t show again",
  closeNoticeHideOnce: "Hide now",
};

export type Dict = typeof en;

const zhCN: Dict = {
  last30Days: "最近 30 天",
  historyRange: "历史范围",
  days7: "7 天",
  days30: "30 天",
  backToday: "回到今日",
  historyLoading: "正在加载 30 天历史…",
  historyFailed: "历史查询失败。",
  historyIncludesToday: "包含查询当日，按本地日历日统计",
  historyOld: "历史快照：已过期或日期、时区已变化，请刷新。",
  selectedDayAll: "所选日期 · 当日 Token 与模型明细（全部 Agent）",
  noModelDetails: "没有可用的模型明细。",
  loadingTitle: "正在加载用量…",
  errorTitle: "无法获取用量",
  errorTimedOut: "用量刷新未在限时内完成，可以重试。",
  errorCancelled: "用量刷新已被取消，可以重试。",
  errorFailed: "无法刷新用量，可以重试。",
  errorTransport: "无法读取本机用量状态，可以重试。",
  errorHint:
    "统计读取的是本机 Coding Agent 的本地使用记录。如果持续失败，请确认这台机器上近期使用过受支持的 Coding Agent。",
  tryAgain: "重试",
  retry: "重试",
  refresh: "刷新",
  refreshing: "刷新中…",
  today: "今日",
  usageFor: (date: string) => `${date} 的用量`,
  staleFailedOld: (date: string) => `刷新失败 — 显示的是 ${date} 的旧数据。`,
  staleFailedRecent: "刷新失败 — 以下数字仍是最近一次成功采集的结果。",
  staleOld: (date: string) => `显示的是 ${date} 的旧数据。`,
  todaySrHeading: "今日 Token 用量",
  todaySrHeadingFor: (date: string) => `${date} 的 Token 用量`,
  // "Tokens" is a unit label; unit wording and exact numeric semantics do not
  // change between languages.
  tokensUnit: "Tokens",
  cachedInput: (pct: string) => `约 ${pct} 缓存输入`,
  emptyToday: "今日没有找到 Agent 用量记录。",
  emptyFor: (date: string) => `${date} 没有找到 Agent 用量记录。`,
  whyNoUsage: "为什么没有用量？",
  emptyHelp:
    "统计只包含所示日期内、记录在这台 Windows 用户账户下的用量。如果你预期有用量，请确认在该时间范围内这台机器上确实使用过受支持的 Coding Agent。",
  tokenBreakdown: "Token 构成",
  shareOfToday: "占今日比例",
  shareOf: (date: string) => `占 ${date} 比例`,
  breakdown: {
    input: "输入",
    output: "输出",
    cacheRead: "缓存读取",
    cacheCreation: "缓存创建",
    reasoning: "推理",
    unclassified: "未分类",
  },
  aboutTitle: "关于这些数字",
  aboutScope: "统计范围。",
  aboutScopeBody:
    "统计来自这台 Windows 用户账户下、所示日期内的 Coding Agent 使用记录（按你的本地时区）。任何数据都不会离开这台机器。",
  aboutTypes: "Token 类型。",
  aboutTypesBody:
    "输入:发送给模型的提示 Token。输出:模型生成的 Token。缓存读取:由服务商提示缓存提供的提示 Token。缓存创建:写入该缓存的提示 Token。推理:部分 Agent 在输入/输出之外报告的隐藏思考 Token。未分类:来源报告的、超出上述类型的部分。",
  aboutCache: "缓存输入占比",
  aboutCacheBody: " = 缓存读取 ÷（输入 + 缓存读取 + 缓存创建）。输出 Token 不在该分母中。",
  aboutCost: "预估成本",
  aboutCostBody:
    "是基于 Token 数量、使用应用内置离线价格表计算的参考估算，不是账单、订阅费用，也不是账户额度或限额。当任一贡献模型缺少价格时，成本显示为不可用，而不是部分合计或伪造的 $0.00。没有用量时，成本标为不适用；仅当有用量且完整估算为零时才显示 $0.00。",
  last7Days: "最近 7 天",
  total: (n: string) => `合计 ${n}`,
  filterByAgent: "按 Agent 筛选",
  filterTrendOnly: "筛选只影响趋势 — 上方今日摘要始终完整。",
  all: "全部",
  moreAgents: (n: number) => `更多 Agent（${n}）`,
  otherAgents: (n: number) => `其他 Agent（${n}）`,
  scaleAll: "柱高按窗口内最大日缩放；切换筛选会重新缩放。",
  scaleAgent: (name: string) => `柱高按 ${name} 的最大日缩放。`,
  dayDetailRegion: "所选日期详情",
  footerLastRefresh: "最近成功刷新",
  footerUpdated: (rel: string) => `更新于 ${rel}`,
  justNow: "刚刚",
  minutesAgo: (m: number) => `${m} 分钟前`,
  hoursAgo: (h: number) => `${h} 小时前`,
  daysAgo: (d: number) => `${d} 天前`,
  tooltipTokensTotal: (n: string) => `Token 合计 ${n}`,
  tooltipOfDay: (pct: string) => `占当日 ${pct}%`,
  includesReasoning: (n: string) => `Agent 总量包含 ${n} 推理`,
  includesUnclassified: (n: string) => `Agent 总量包含 ${n} 未分类 Token`,
  cacheAcrossModels: (pct: string, models: number) =>
    `跨 ${models} 个模型约 ${pct} 缓存输入`,
  modelComp: { in: "输入", out: "输出", cacheRead: "缓存读取", creation: "缓存创建" },
  costValue: (usd: string) => `预估成本 ${usd}`,
  costUnavailable: "预估成本不可用",
  costMissingPrices: "预估成本不可用 — 缺少模型价格",
  costNoUsage: "预估成本不适用 — 无用量",
  deltaVsYesterdayFullDay: "较昨日全天",
  deltaVsPreviousDay: "较前一日",
  deltaVsPreviousWeek: "较前一周",
  deltaNoYesterday: "— 无前一日数据",
  deltaNoUsageYesterday: "— 昨日无用量",
  ariaTotal: (n: string) => `Token 合计 ${n}。`,
  ariaAgentShare: (name: string, n: string, pct: string) =>
    `${name}:${n} Token(${pct}%)。`,
  ariaAgentOfDay: (name: string, n: string, pct: string) =>
    `${name} ${n} Token，占当日 ${pct}%。`,
  settingsTitle: "偏好设置",
  languageLabel: "语言",
  languageSystem: "跟随系统",
  startWithWindows: "随 Windows 启动",
  startHiddenToTray: "启动时隐藏到托盘",
  settingsSaveFailed: "此偏好设置未能保存。",
  // 今日区构成条(review B1 + A3)与日详情关闭按钮(review C2)。
  compDimensions: "构成维度",
  compByAgent: "按 Agent",
  compByType: "按类型",
  compExpand: "展开明细",
  compCollapse: "收起明细",
  compBarLabel: "今日 Token 构成，点击展开或收起明细",
  closeDetail: "关闭详情",
  // 30 天周聚合(review B2)与成本负态降权(review A2)。短标记的完整原因
  // 保留在 tooltip 与"关于这些数字"中;成本三态语义不塌缩。
  aggMode: "聚合方式",
  aggWeek: "按周",
  aggDay: "按日",
  scaleAllWeek: "柱高按窗口内最大周缩放；切换筛选会重新缩放。",
  costNa: "成本 n/a ⓘ",
  selectedWeekAll: "所选周 · 当周 Token 与模型明细（全部 Agent）",
  closeNoticeTitle: "仍在托盘运行",
  closeNoticeBody:
    "关闭窗口后，Coding Agent Monitor 会继续在系统托盘中运行。可从托盘图标重新打开主界面，或从托盘菜单退出。",
  closeNoticeAcknowledge: "知道了，不再提示",
  closeNoticeHideOnce: "立即隐藏",
};

const DICTIONARIES: Record<Language, Dict> = { en, "zh-CN": zhCN };

export function dictFor(lang: Language): Dict {
  return DICTIONARIES[lang];
}
