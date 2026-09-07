# Coding Agent Monitor — Agent Instructions

## Read First

Before changing code, read `README.md`, this file, and
`docs/IMPLEMENTATION_PLAN.md`. Complete only the assigned task. For a versioned
phase, also read that version's development and acceptance plan completely.
Phase records describe their dated baselines; use the current-status index in
`docs/IMPLEMENTATION_PLAN.md` to find subsequent decisions.

## Project Goal

Maintain a lightweight, local Windows Coding Agent Token Monitor that stays
quiet in the system tray. v0.3 supports 17 agents through vendored ccusage
v20.0.20 plus the pinned Antigravity downstream port. The released support
statement is Windows 11 x64 only; Windows 10 and the full non-ASCII-profile GUI
scenario remain unverified. See `docs/V0.3_RELEASE_GATE_DECISION.md` for waivers.

## Historical MVP Release Scope

The v0.1.0 release permits exactly:

1. Automatic Claude Code and Codex token collection.
2. Today's usage and a seven-day trend.
3. A Windows system tray summary with show/hide and exit actions.

## Engineering Principles

- Prefer simple implementations.
- Do not introduce abstractions for hypothetical future requirements.
- Do not add dependencies unless necessary.
- Do not redesign unrelated code.
- Keep background CPU and memory usage low.
- Windows is the only MVP platform.
- Frontend code must never parse ccusage raw output.
- Rust owns native process, tray, filesystem, and SQLite responsibilities.
- React owns presentation and simple UI state.
- Keep TypeScript and Rust usage contracts aligned.
- Keep vendored API/output conversion inside `src-tauri/src/collector/ccusage.rs`.
- Keep public-summary normalization inside `src-tauri/src/sidecar/adapter.rs`
  (the module name is historical; production uses `normalize_snapshot`).
- Avoid premature optimization and speculative architecture.
- Fix root causes rather than adding workarounds where practical.
- Never present fixture or mock data as real usage.

## Scope Guard

v0.1, v0.2, and v0.3 implementation/release work is historical; do not restart
completed phases. Maintenance and documentation fixes may follow an explicit
user assignment. New product work requires an explicit assignment and an
applicable versioned plan; do not infer it from an old unchecked item.

Unless explicitly assigned, do not proactively implement login,
cloud sync, API proxies, MCP, AI analysis, agent benchmarks, project analytics,
session exploration, burn rate, notifications, plugins, non-Windows platforms,
more agents, complex settings, enterprise dashboards, or a generic provider
framework.

## Architecture Boundaries

```text
Local agent records (read-only)
        -> vendored ccusage inside the product EXE's isolated worker
        -> CAM-owned typed snapshot
        -> Rust normalize_snapshot adapter
        -> UsageSummary
        -> Tauri command / tray event
        -> React dashboard / Windows tray
```

- `src/types/usage.ts` and `src-tauri/src/usage/mod.rs` are the public contract.
- `collector/ccusage.rs` is the boundary to vendored APIs; vendor types must not
  leak into the worker protocol or public usage contract.
- `sidecar/adapter.rs::normalize_reports` retains v0.2 JSON decoding only for
  opt-in shadow audits. Do not reintroduce external sidecar lookup or fallback.
- Rust supervises `current_exe()` workers; React only invokes project commands
  and listens for normalized usage events.
- SQLite is a local cache only. Do not add business tables without measured need.
- Keep one window and one feature area; do not add routing or a sidebar.

## Development Rules

- Inspect existing code before editing and preserve unrelated work.
- Do not expand scope when an implementation-plan issue is discovered; record it.
- Add or update focused tests for contract and adapter behavior.
- After changes, run the relevant subset of:
  - `pnpm lint`
  - `pnpm typecheck`
  - `pnpm test`
  - `pnpm test:e2e` (for UI changes)
  - `pnpm vendor:verify` (for collector, vendor, or build changes)
  - `pnpm build`
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `pnpm tauri build`
- Report commands that were not run or were blocked; never claim an unrun check passed.
- Keep commits and tasks small enough for the next agent to review quickly.
