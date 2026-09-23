# Implementation Plan

Status legend: `[x]` completed for the stated phase; `[ ]` was outstanding at
the time of that record. Neither checkbox implies a current release gate PASS.

> **Current status (2026-09-23): v0.5.0 release.** All three product manifests
> are `0.5.0`. The production path uses vendored ccusage v20.0.20 plus the
> Antigravity downstream port and Claude Desktop local-agent discovery in the
> same-EXE worker, covering 18 agents. No external ccusage executable is staged.
> Official support is Windows 11 x64 only. Installers are unsigned; the
> developer-tools-free offline host, real logon/reboot and sleep/resume, and
> full non-ASCII-profile GUI scenarios were not run for v0.5.0. The maintainer
> directed publication with these gaps disclosed. See the
> [v0.5 verification](V0.5_RELEASE_VERIFICATION.md) and
> [release notes](V0.5_RELEASE_NOTES.md); older decisions remain historical.
>
> **This file's Phases 1–10 are historical v0.1/v0.2 records.** Phase 10 was
> executed on 2026-08-28 and v0.2.0 was published on 2026-08-29. References below
> to sidecar executables, staging, or the initial Claude/Codex UI describe those
> phases, not current development instructions. v0.1, v0.2, and v0.3 each record
> the full non-ASCII-profile GUI scenario as **WAIVED / NOT RUN**, never PASS.
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

> **2026-09-10 historical work:** user assigned T07/T08 grouped commits, save/check
> fixes, clean validation, native acceptance and local v0.4.0 packaging. Current
> manifests are `0.4.0`; the published version remains v0.3.0. See the
> [fresh verification record](V0.4_RELEASE_VERIFICATION.md) and
> [candidate release notes](V0.4_RELEASE_NOTES.md). T09's product feature and T10
> remain inactive. Historical no-bump restrictions below describe prior tasks;
> local packaging is explicitly authorized by this new assignment.

## Current documentation index

| Purpose | Document |
| --- | --- |
| Current behavior and setup | [README](../README.md), [CONTRIBUTING](../CONTRIBUTING.md), [AGENTS](../AGENTS.md) |
| v0.1 verification history | [RELEASE_VERIFICATION](RELEASE_VERIFICATION.md) |
| v0.2 implementation and verification | [Plan](V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md), [verification](V0.2_RELEASE_VERIFICATION.md) |
| v0.3 migration decisions and phase mapping | [v0.3 plan](V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md) |
| v0.3 production architecture | [Phase 4B](V0.3_PHASE4B_PRODUCTION_SWITCH.md), [Phase 5 cleanup and acceptance](V0.3_PHASE5_RELEASE_CANDIDATE.md) |
| v0.3 final gates, waivers and publication | [Gate decision](V0.3_RELEASE_GATE_DECISION.md), [release checklist](OPEN_SOURCE_RELEASE_CHECKLIST.md), [release notes](V0.3_RELEASE_NOTES.md) |
| v0.4 development history | [v0.4 development and acceptance plan](V0.4_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md) — T00–T06 complete; T07 implemented (2026-09-07; native/performance limitations in §11.8); T08 research completed and revised ([feasibility study R1](QUOTA_MONITOR_FEASIBILITY.md), §11.9); T09–T10 inactive candidates |
| v0.5 release identity, gates and disclosed limits | [Verification](V0.5_RELEASE_VERIFICATION.md), [release notes](V0.5_RELEASE_NOTES.md), [manual walkthrough](V0.5_MANUAL_WALKTHROUGH_2026-09-18.md) |
| Documentation audit and remaining maintenance observations | [2026-09-06 audit](DOCUMENTATION_AUDIT_2026-09-06.md) |

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

## Post-MVP v0.2 — Completed implementation and verification history

Phase 6–9 were internal v0.2 planning labels implemented while the manifest
still read `0.1.0`. Phase 10 was subsequently completed and the version bumped
to `0.2.0` for that release; the current manifest is `0.3.0`.
The detailed sequence, contract decisions, test matrix, performance baseline,
visual boundaries and historical outcomes live in
[`V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md`](V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md).

v0.2 phase outcomes:

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
   viewport, installer, offline, and non-ASCII-profile acceptance. **EXECUTED
   (2026-08-28)**; actual results, unrun items, and the non-ASCII-profile waiver
   are in [V0.2_RELEASE_VERIFICATION.md](V0.2_RELEASE_VERIFICATION.md).

Phases 6–10 are complete historical work units. Completion of a verification
task does not turn its waived or unrun scenarios into technical passes.

## Next Executable Task

The v0.4 planning task T00 and the core implementation tasks T01–T06 are
complete; their evidence and unrun native scenarios are recorded in the v0.4
plan §11.2–§11.7. T07 was assigned and implemented on 2026-09-07; see §11.8 and its acceptance
record for completed checks and unverified native/performance scenarios.
T08 feasibility research was assigned and completed on 2026-09-07, and revised under T08-R1, with review corrections on 2026-09-08 (see §11.9 and docs/QUOTA_MONITOR_FEASIBILITY.md; research-only, no production code changes or quota productization authorized). T09–T10 remain inactive candidates and never become authorized merely because their dependencies are complete — each starts only when the user sends its standalone Prompt. Core-chain completion is not a v0.4
release candidate and does not authorize a version bump, tag, or publication.
See the [v0.4 plan](V0.4_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md) for the dependency
table, semantic boundaries, acceptance matrix and task result locations.

v0.3 migration and publication remain complete historical work. Do not restart
v0.2 Phase 10, recreate deleted sidecar setup, or treat historical unchecked
items as new work.

Windows 10 lifecycle and the full non-ASCII-profile GUI scenario remain coverage
gaps. Any future validation must record fresh evidence; current waivers do not
establish technical support for those scenarios.
