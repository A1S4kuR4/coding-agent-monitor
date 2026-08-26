# Coding Agent Monitor — Agent Instructions

## Read First

Before changing code, read `README.md`, this file, and
`docs/IMPLEMENTATION_PLAN.md`. Complete only the currently assigned plan item.
When the assigned item is a v0.2 phase, also read
`docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md` completely.

## Project Goal

Build a lightweight Windows 10/11 Coding Agent Token Monitor. It runs locally,
stays quiet in the Windows system tray, and reports Claude Code and OpenAI Codex
token usage.

## MVP Release Scope

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
- Keep ccusage-specific fields inside `src-tauri/src/sidecar`.
- Avoid premature optimization and speculative architecture.
- Fix root causes rather than adding workarounds where practical.
- Never present fixture or mock data as real usage.

## Scope Guard

Until the v0.1.0 release gate in `docs/IMPLEMENTATION_PLAN.md` is closed, do not
mix post-MVP work into the release task. After that gate, implement post-MVP work
only when the user explicitly assigns a phase from
`docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md`.

Unless such a phase is explicitly assigned, do not proactively implement login,
cloud sync, API proxies, MCP, AI analysis, agent benchmarks, project analytics,
session exploration, burn rate, notifications, plugins, non-Windows platforms,
more agents, complex settings, enterprise dashboards, or a generic provider
framework.

## Architecture Boundaries

```text
Claude Code / Codex logs
        -> ccusage native sidecar
        -> Rust sidecar adapter
        -> UsageSummary
        -> Tauri command
        -> React dashboard
```

- `src/types/usage.ts` and `src-tauri/src/usage/mod.rs` are the public contract.
- `src-tauri/src/sidecar` is the only place that knows ccusage JSON shapes.
- Rust must invoke and supervise the sidecar; React only invokes project commands.
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
  - `pnpm build`
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `pnpm tauri build`
- Report commands that were not run or were blocked; never claim an unrun check passed.
- Keep commits and tasks small enough for the next agent to review quickly.
