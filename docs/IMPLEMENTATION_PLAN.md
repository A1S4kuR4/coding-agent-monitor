# Implementation Plan

Status legend: `[x]` scaffolded, `[ ]` implementation remains.

> **Current status: all phases through Phase 9 are implemented** (ccusage 20.0.20
> sidecar packaged, real data path, real dashboard, Windows tray, MSI/NSIS
> installers, the v0.2 Phase 6 unified sidecar snapshot, the Phase 7 dynamic
> open-string agent contract + display list, the Phase 8 estimated cost /
> cached-input share / collection time / stale-data recovery, and the Phase 9
> Reading-Surface + responsive refinement). The per-item checkboxes below are
> historical implementation records, not an outstanding to-do list. Actual
> build/install/launch/uninstall outcomes are recorded in
> [`RELEASE_VERIFICATION.md`](RELEASE_VERIFICATION.md); v0.2 Phase outcomes are
> recorded in [`V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md`](V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md).
>
> **Phases 6–9 were implemented (2026-08-25). For the v0.1.0 pre-release, the
> Chinese-profile full-GUI gate (Gate 0) is now explicitly `WAIVED / NOT RUN`:
> the scenario remains unverified and is not a PASS. Phase 10 retains complete
> non-ASCII-profile coverage as a future v0.2 release-verification task.**
>
> **Phase 7–9 review fixes (2026-08-25):** the 8 defects found in review — horizontal
> overflow/clipping at 420×560 & 125%/150% DPI; WCAG AA contrast on the dark primary
> and stale-retry buttons; the explicit Rust u64→JS safe-integer policy; async
> `onFocusChanged`/`usage-updated` registration leaks + post-unmount state updates;
> the Phase 8 component/state tests; unified daily `totalTokens` ⬄ `agents[]` sum
> validation; the zero-token trend-bar height; and core-stat Mono track — were fixed
> and are recorded in `docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §12`, together
> with the actual command results and unrun GUI items. References there to an
> open Gate 0 are historical and superseded by the v0.1.0 pre-release waiver.
> A second review found two P2 issues — per-row (vs. first-wins) validation of
> duplicate-date `totalTokens` ⬄ `agents[]` sums, and a deferred-registration
> race test for focus/tray listeners — which were fixed, tested, and recorded
> in `docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §12.6`.**

## Phase 1 — Project Scaffold

**Goal:** establish a minimal Tauri 2 + React + TypeScript + Rust desktop app.

**Main files:** `package.json`, `src/`, `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json`, `src-tauri/src/error.rs`, `src-tauri/src/db/mod.rs`.

- [x] React/Vite/TypeScript project and strict type checking.
- [x] Tauri command boundary and serializable application errors.
- [x] One resizable main window; closing hides it instead of ending the tray process.
- [x] SQLite opens in Tauri's per-user application-data directory with no business tables.

**Acceptance:** frontend builds; Rust checks/tests pass on a Windows machine with the
Tauri prerequisites; starting with `pnpm tauri dev` opens one window and creates an
empty `usage-cache.sqlite3` safely under the user profile, including non-ASCII paths.

**SQLite decision:** keep initialization only. ccusage already returns today's and
seven-day aggregates. Add a minimal cache table only if Phase 2 measurements show
that repeated sidecar execution is too expensive for tray refreshes.

## Phase 2 — ccusage Data Path

**Goal:** replace fixture-backed command output with supervised native ccusage output.

**Main files:** `src-tauri/src/sidecar/`, `src-tauri/src/commands/usage.rs`,
`src-tauri/binaries/`, `src-tauri/tauri.conf.json`, adapter fixtures.

- [x] Confirmed focused JSON shapes with ccusage 20.0.20: both commands provide
  `daily[].date` and `daily[].totalTokens`; other fields differ and are ignored.
- [x] Thin adapter merges `ccusage claude daily --json --offline` and
  `ccusage codex daily --json --offline` into `UsageSummary`.
- [x] Fixtures and normalization tests cover missing source data, empty reports,
  seven-day gap filling, and malformed JSON.
- [x] Pin one tested ccusage version and its `@ccusage/ccusage-win32-x64` native
  package. Copy `ccusage.exe` to `src-tauri/binaries/ccusage-<target-triple>.exe`.
- [x] Add `bundle.externalBin` and initialize `tauri-plugin-shell` only after the
  binary exists, so scaffold builds never reference a missing executable.
- [x] In Rust, execute Claude and Codex focused commands sequentially with local
  timezone date bounds. Capture stdout and stderr separately, validate exit status
  and UTF-8 JSON, set a timeout, and ensure the child is killed/dropped on timeout.
- [x] Map missing binary, non-zero exit, malformed JSON, and no-agent-data cases to
  stable `AppError` codes and user-readable messages. Never log raw local records.
- [x] Remove the UI `Mock data` badge only when packaged and development sidecars
  both pass tests on Windows x64.

**Acceptance:** the installed `.exe` works without Node, pnpm, Bun, or network access;
it reads paths under normal and Chinese Windows user names; no child process remains
after success, failure, timeout, app exit, or repeated refresh. Claude-only and
Codex-only machines return zero for the missing agent rather than failing the report.

## Phase 3 — Main UI

**Goal:** show only today's total, Claude/Codex split, and the last seven days.

**Main files:** `src/App.tsx`, `src/App.css`, `src/features/usage/`,
`src/lib/usage-api.ts`, `src/types/usage.ts`.

- [x] Restrained single-screen dashboard skeleton and dependency-free bar trend.
- [x] Loading, empty, error, and retry states.
- [x] Token-number formatter with frontend tests.
- [x] Connect refresh to the real command and verify labels with local timezone data.

**Acceptance:** the view contains no navigation or non-MVP metrics, remains readable
at the configured minimum window size, and never imports ccusage types or fields.

## Phase 4 — Windows Tray

**Goal:** keep the application available from the Windows notification area.

**Main files:** `src-tauri/src/tray/mod.rs`, `src-tauri/src/lib.rs`.

- [x] Tray icon, left-click show, menu open, hide-on-close, and exit skeleton.
- [x] Replace mock tooltip/menu text with today's normalized total and Claude/Codex
  split after Phase 2; refresh at a conservative measured interval or on user action.
- [x] Verify the Exit action terminates the app and any active sidecar child.

**Acceptance:** the main window can be shown/hidden repeatedly, tray text reflects the
same `UsageSummary` as React, idle resource use stays low, and Exit leaves no process.

## Phase 5 — Release Verification

**Goal:** prove the packaged Windows application is self-contained and failure-safe.

**Main files:** package/build configuration plus a short release verification record.

- [x] Run lint, typecheck, frontend tests/build, Cargo check/test, Tauri dev, and
  production Tauri build on Windows.
- [x] Test standard paths and non-ASCII native data paths; the full GUI run from
  a real Chinese-named Windows profile remains the release gate below.
- [x] Test neither agent installed, empty logs, Claude-only, and Codex-only data.
- [x] Test malformed stdout, non-zero sidecar exit, timeout, and missing sidecar.
- [x] Confirm packaged executable starts without developer tools or network access.

**Acceptance:** all checks are recorded with actual outcomes; the installer contains
the target-triple ccusage binary; Task Manager shows no residual child process.

## Post-MVP v0.2 — Implemented internal phases and remaining verification

Phase 6–9 were internal v0.2 planning labels and have already been implemented,
even though the application manifest remains `0.1.0` for this pre-release candidate.
The detailed sequence, contract decisions, test matrix, performance baseline,
visual boundaries, historical outcomes, and remaining release verification live in
[`V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md`](V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md).

Current phase status:

1. **Phase 6 — Unified sidecar snapshot:** replace two focused child processes with
   one `daily --json --offline --by-agent` process while keeping the current public
   `UsageSummary` stable for parity verification. **DONE (2026-08-25).** The later
   pinned Antigravity compatibility bridge adds one focused sidecar without changing
   the official pinned binary's authority for the unified snapshot.
2. **Phase 7 — Dynamic agent contract:** replace the closed Claude/Codex enums with
   an open string ID plus display name and render only agents active today. **DONE
   (2026-08-25)** — open-string `id` + Rust-produced `displayName`, per-day dynamic
   `agents[]` with stable sort and unknown-id fallback, React today list + empty state,
   bounded dynamic tray summary; see `V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §9`.
3. **Phase 8 — Lightweight insight and freshness:** estimate USD cost, cache-share,
   collection time, stale-data recovery, and tray-to-window refresh events. Do not expose
   unused model-breakdown data. **DONE (2026-08-25)** — day-level `estimatedCostUsd` +
   `cacheReadShare` computed in the Rust adapter, `collectedAt` stamped by the runner,
   React renders cost + cache share + "Updated Xm ago", stale banner, and a tray→window
   `usage-updated` event; see `V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §10`.
4. **Phase 9 — Reading-surface refinement:** apply the transferable dual-surface
   typography, accent, hairline, and responsive rules without adding routes or
   pretending the dashboard is a dark-stage flow. **DONE (2026-08-25)** — pure
   `App.css` refactor: CSS tokens (paper/ink/terracotta/indigo/moss/hairline/spacing),
   Serif/Sans/Mono stacks (system only), warm core value + cool trend + Moss
   reserved for success, compact height query, `:focus-visible` Indigo ring,
   reduced-motion, natural-scroll at 200% zoom; see `V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §11`.
5. **Phase 10 — v0.2 release verification:** repeat automated, real-data, process,
   viewport, installer, offline, and non-ASCII-profile acceptance.

Phases 6–9 are complete historical implementation units. Phase 10 is still a separate
future v0.2 release-verification task and must record actual results rather than infer
PASS from earlier phase checks.

## Next Executable Task

Phases 6–9 (unified sidecar snapshot, dynamic Agent contract, estimated cost/cache/collection
time/stale-data recovery, and Reading-Surface refinement) were executed and recorded on
2026-08-25. The next v0.2 task is **Phase 10**
(`V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md`):
v0.2 release verification (repeat full automated, real-data, process, installer, offline and
non-ASCII acceptance; write a new `docs/V0.2_RELEASE_VERIFICATION.md`, do not overwrite v0.1
history). That file must not be created until Phase 10 is actually executed.

The Chinese-profile full-GUI gate (`RELEASE_VERIFICATION.md §9–§10`, Gate 0) is
**WAIVED / NOT RUN for the v0.1.0 pre-release**, not technically verified. The same
scenario remains an explicit Phase 10 coverage requirement before a future v0.2 release
can be evaluated.
