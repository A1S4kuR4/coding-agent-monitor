# Release Verification Record — Coding Agent Monitor v0.1.0

**Date:** 2026-08-24
**Environment:** Windows 11 Home China, x64, ASCII-named user profile (username redacted)
**Build reproducibility:** sidecar pinned to ccusage 20.0.20, `@ccusage/ccusage-win32-x64`, exe SHA-256 `3fd2f3b9…245bc`.

All outcomes below are actual, run live against the built artifacts. Any item not completed is explicitly identified rather than assumed.

---

## 1. Automated check suite — PASS

| Check | Result |
|---|---|
| `cargo test` (src-tauri) | **PASS** — 10 passed, 0 failed, 1 ignored (the ignored one is the real-data smoke, run explicitly below) |
| `cargo build --release` | **PASS** |
| `pnpm lint` | **PASS** |
| `pnpm typecheck` | **PASS** |
| `pnpm test` (vitest) | **PASS** — 1 file, 2 tests |
| `pnpm tauri build` | **PASS** — produced MSI + NSIS bundles |

Real-data smoke (`cargo test --release -- --ignored collects_real_usage_into_a_seven_day_summary`): **PASS**
- Exact personal usage values are redacted; a valid 7-day array was produced from local logs with no Node/pnpm/network.

## 2. Data-path scenarios — PASS (unit-level + live)

Covered by existing tests in `src-tauri/src/sidecar/`:

| Scenario | Coverage | Result |
|---|---|---|
| No agent installed / empty logs | live: empty profile → both `claude`/`codex daily --json` return `{"daily":[]}`, exit 0; adapter `returns_zeroes_when_both_agents_have_no_data` | **PASS** |
| Claude-only / Codex-only (one agent empty) | adapter `accepts_an_empty_agent_report` merges one-empty + one-full correctly | **PASS** |
| Malformed JSON | adapter `rejects_malformed_json_with_an_actionable_code`, `rejects_a_malformed_daily_date` | **PASS** |
| Timeout | runner `reaps_a_child_that_times_out` (kills + reaps on 500 ms bound) | **PASS** |
| Abnormal exit (non-zero) | runner `surfaces_a_non_zero_exit_status` (`exit 7`) | **PASS** |
| Sidecar missing | `build.rs` panics with a clear message when the target triple binary is absent | **PASS** (proven at build time) |
| Two agents normalized + missing days filled | adapter `normalizes_focused_reports_and_fills_missing_days` | **PASS** |

Live validation: `USERPROFILE`/`HOME` pointed at an empty dir → both agents return empty, exit 0 (no-agent / empty-logs). Same check against a **Chinese-named directory** (`用户测试目录`) → ran without error, exit 0.

## 3. Ordinary + Chinese user-profile paths — PASS

- **Ordinary (ASCII) path:** app built, launched, and read real usage (see smoke test above).
- **Chinese (non-ASCII) path `用户测试目录`:** ccusage `claude` and `codex daily --json --offline` both succeed (empty, exit 0) — no path-encoding crash. A Rust regression test also creates, reopens, verifies WAL mode, and removes the real SQLite file inside a Chinese-named directory. No separate GUI run under a Chinese profile was performed; the native data-path components were verified directly.

## 4. Installer build — PASS

Fresh `pnpm tauri build` produced:
- `src-tauri/target/release/bundle/msi/Coding Agent Monitor_0.1.0_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/Coding Agent Monitor_0.1.0_x64-setup.exe`

**MSI content verified** via `msiexec /a` admin-extract (no elevation needed for extraction):
- contains `ccusage.exe` → SHA-256 `3fd2f3b9…245bc` = exact staged binary ✅
- contains `coding-agent-monitor.exe` (10.7 MB) ✅

## 5. Install → launch → uninstall

### NSIS — FULL CYCLE VERIFIED ✅
- **Install:** silent per-user (`/S /currentuser`), exit 0 → `%LOCALAPPDATA%\Coding Agent Monitor\`, registry `Uninstall` entry + Start Menu shortcut created. Installed `ccusage.exe` SHA-256 = `3fd2f3b9…245bc` ✅
- **Launch:** exe started and stayed alive; the sidecar spawned at setup and was reaped; no residual `ccusage.exe` after it ran. No panic.
- **Uninstall:** silent `uninstall.exe /S` → install dir removed, registry entry removed, Start Menu shortcut removed, no residual `coding-agent-monitor.exe` / `ccusage.exe` processes.

### MSI — FULL CYCLE VERIFIED ✅ (run elevated on 2026-08-24)
- **Install (elevated):** `msiexec /i … /qn` via `Start-Process -Verb RunAs` (UAC approved) → exit **0**. Tauri's MSI defaults to a per-user install, so files landed under `%LOCALAPPDATA%\Coding Agent Monitor\` even when elevated. Installed `ccusage.exe` SHA-256 = `3FD2F3B9…245BC` ✅; the build-specific product code is intentionally omitted from the public record.
- **Launch:** exe started, stayed alive 7 s (no crash), sidecar spawned at setup and reaped; no residual `ccusage.exe` after it ran.
- **Uninstall (elevated):** `msiexec /x {1DEE55D9-…} /qn` → exit **0**. Install dir, registry `Uninstall` entry, and Start Menu shortcut all removed; **zero** residual `coding-agent-monitor.exe` / `ccusage.exe` processes.
- Earlier exit-103 on the non-elevated attempt was an environment elevation-gating quirk, not a package defect; with elevation the MSI behaved normally end-to-end.

> Note: an earlier build-time `Warn` flagged the bundle identifier `com.codingagentmonitor.app` for its `.app` suffix (discouraged on macOS). Resolved at release finalization by switching the identifier to `com.codingagentmonitor` — see the release checklist. Windows-only target.

---

## 6. Residual-process hygiene — PASS
After the app run and after uninstall, `tasklist` confirmed **zero** `coding-agent-monitor.exe` or `ccusage.exe` processes. Exit path kills in-flight sidecars via `taskkill /F /T /PID` (logic covered by unit tests).

---

## 7. Post-backlog review fixes (2026-08-24)

Three P1, three P2, and one P3 findings from the review were addressed and re-verified:

| # | Finding | Fix | Verification |
|---|---|---|---|
| P1-1 | Dashboard data goes stale when reopened from tray | `App.tsx` refetches on window focus via `getCurrentWindow().onFocusChanged` | lint / typecheck / vitest pass; shipped in rebuilt installers |
| P1-2 | 500ms timeout test really took ~40s (grandchild held the pipe) | `await_capture` kills the whole tree on timeout/error (`taskkill /F /T` via `kill_tree`); elapsed assertion added | suite dropped **39.4s → 0.91s**; assertion `< 5s` passes |
| P1-3 | Refresh could spawn concurrent/unbounded sidecars | `runner.rs` `COLLECT_LOCK` serializes collect (concurrency = 1); `tray` `REFRESH_IN_PROGRESS` single-flight; removed duplicate left-click refresh | cargo test green; code-reviewed paths |
| P2-4 | Release EXE embedded an absolute development-machine path | `CARGO_MANIFEST_DIR` fallback gated to `#[cfg(debug_assertions)]`; release uses only the packed `ccusage.exe` beside the exe | release+rebuilt EXE grepped — **no dev path**; unpackaged release launches without panic on `sidecar_missing` |
| P2-5 | Every refresh scanned the full log history | `--since <today-6>` bound added to both agent commands (7-day window) | real-data smoke runs in ~0.35s, still returns valid 7-day summary |
| P2-6 | Docs still described a mock skeleton | Updated `README.md` and `IMPLEMENTATION_PLAN.md` phase status to implemented state | read confirm |
| P3-7 | `cargo fmt --check` failed | `cargo fmt` applied; `cargo fmt --check` clean | clean |

**Re-run suite after fixes:** `cargo test` 9 passed 1 ignored; `cargo clippy --all-targets -- -D warnings` clean; `pnpm lint/typecheck/test` pass; fresh `pnpm tauri build` produced updated MSI + NSIS. NSIS per-user cycle was re-verified after the rebuild: install → launch (alive, sidecar reaped) → stop → remove, zero residual processes. **MSI elevated install → launch → uninstall also completed** (see §5) — both installers now have a full pass. Only remaining caveat: no GUI run was done under a full Chinese-language Windows user account; non-ASCII paths were exercised at the binary level (Section 3) and the MSI/NSIS installs used ASCII-named paths.

> Superseded where noted by §8: a second review found that the concurrency cap
> prevented overlap but still allowed a focus-driven sequence of sidecar runs, and
> that the release-mode real-data smoke could no longer locate its test sidecar.

## 8. Second-review corrections (2026-08-24)

| Finding | Correction | Re-verification |
|---|---|---|
| Focus changes could turn sidecar refreshes into a continuous serialized loop | Windows children now use `CREATE_NO_WINDOW`; Rust retains the latest success or failure for two seconds so tray and dashboard requests share one collection; React drops focus refreshes while one is in flight | Before: **25 distinct ccusage PIDs in 12 s**, still growing. After a fresh release build: **2 startup PIDs in 8 s**, both in the first second, maximum concurrent 1, no later PIDs, zero residual processes after stop |
| The documented release real-data smoke failed with `sidecar_missing` after the production fallback was removed | The staged path is available only under `debug_assertions` or `cfg(test)`; normal release binaries still know only the packaged sibling `ccusage.exe` | Exact documented release command passes in **about 0.4 s** and produces a seven-day summary; the release application contains no repository path |
| Shutdown could occur between child spawn and PID registration | The runner rechecks shutdown after registration and kills/reaps immediately when teardown won the race | Source-path audit, full Rust suite, and runtime stop with zero residual processes |
| A tray refresher thread failure or panic could leave its single-flight flag set | An RAII guard resets the flag on every thread exit, with explicit reset when thread creation itself fails | Strict Clippy and full Rust tests pass |
| The implementation plan still directed the next agent to an already completed Phase 2 task | Replaced it with the sole recorded coverage caveat: full-GUI verification from a Chinese-profile Windows account | Read-back review |

Second-review command results: `pnpm lint`, `pnpm typecheck`, `pnpm test`, frontend
production build, `cargo fmt --check`, `cargo test` (10 passed, 1 ignored; test body
0.93 s), strict Clippy, release real-data smoke, and fresh `pnpm tauri build` all pass.
The freshly rebuilt NSIS was also reinstalled after these corrections: install exit 0,
packaged sidecar checksum matched, installed-app sampling showed only the two first-second
sidecars, and silent uninstall removed the directory, registry entry, shortcut, and all
processes. The MSI and NSIS full-cycle results in §5 remain the authoritative installer
record; the fresh MSI was rebuilt from the same verified release executable and sidecar.

---

## 9. Gate-0 (Chinese-profile full-GUI) re-checked 2026-08-25 — NOT COMPLETE / blocked by environment

Authoritative record of a fresh gate check on this date. This session re-confirmed that the
v0.1 Chinese-named Windows user-path release gate is **still open**. It was **not** executed,
and **no v0.2 (Phase 6+) code or data-path change was made** in this session.

### 9.1 Gate definition (from `IMPLEMENTATION_PLAN.md` and `V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md` Gate 0)

From a **real Windows account whose user-profile path contains Chinese characters**:
install one freshly built package → launch the **full GUI** → verify SQLite initialization and
both ccusage reports complete → uninstall, then record the actual result.

### 9.2 Actual environment facts gathered this session (live, non-speculative)

- The current session used an ASCII-named user profile and was interactive but
  **non-elevated**. No suitable non-ASCII-profile test account was available.
- Installer artifacts for both NSIS and MSI were present. Local usernames,
  machine account inventory, profile paths, and build timestamps are omitted
  from this public record because they do not affect the result.

### 9.3 Result

**Blocked / not run.** The gate's decisive step — log into a real Chinese-named Windows account
and launch & visually verify the **full GUI** (install → GUI → SQLite + both ccusage reports →
uninstall) — requires an interactive console logon session for such an account. This automated,
non-elevated session cannot create or log into a non-ASCII-named logon account (needs
elevation and an interactive GUI session) and cannot visually verify a GUI. The native data-path
components under Chinese-named paths were already exercised at the binary level in §3 (ccusage
under `用户测试目录`; SQLite WAL/init/remove inside a Chinese-named directory), so §3 remains the
extent of Chinese-path coverage.

### 9.4 Next executable task (unchanged)

A human at the console must create/log into a Chinese-named Windows user account, install the
freshly built package (`...x64-setup.exe`), launch the full GUI, confirm SQLite init and both
ccusage reports succeed, then uninstall — and record the real outcome here. Only after that gate
records a **PASS** may v0.2 Phase 6 begin. This session made **no** v0.2 changes.
