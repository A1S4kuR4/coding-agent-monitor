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

## 9. Gate-0 (Chinese-profile full-GUI) re-checked 2026-08-25 — historical NOT RUN record

This section preserves the result as it stood on 2026-08-25. The check was **not** executed
in that session. Its then-current statements about no Phase 6+ work and the ordering of later
phases are historical only and are superseded by §10: Phase 6–9 were subsequently completed,
and the maintainer waived this gate for the v0.1.0 pre-release without treating it as a PASS.

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

### 9.4 Historical next-step statement (superseded)

At the time, the next proposed task was a human console run from a Chinese-named Windows
profile before Phase 6. That sequencing statement is superseded: Phase 6–9 are complete, while
the GUI scenario itself remains untested. For the v0.1.0 pre-release its disposition is
**WAIVED / NOT RUN**, not PASS; Phase 10 retains non-ASCII-profile coverage as a future v0.2
release-verification task.

---

## 10. Finalization update — 2026-08-26

This update records release-document finalization facts without replacing the dated results
above. It validates candidate contents and metadata; it does **not** claim that the final
installer runtime cycle was repeated.

### 10.1 Source and CI identity

- Exact candidate commit: `08a21c0177cd1ecc902584b94262d411eaf6ccaa`.
- Local `HEAD`, `origin/main`, and GitHub `main` all resolve to that commit.
- Tauri identifier: `com.codingagentmonitor`; the manifest version remains `0.1.0`.
- Latest `main` CI: [run 32942584400](https://github.com/A1S4kuR4/coding-agent-monitor/actions/runs/32942584400),
  completed successfully. Frontend lint, typecheck, tests, and build passed as separate steps;
  Vitest reported **10 files / 63 tests passed**. Rust formatting and strict lint passed;
  Rust tests reported **37 passed / 1 ignored** (the ignored test reads real local usage).
- No Git tag or GitHub Release existed when this update was recorded. The intended publication
  remains a GitHub **Pre-release**, not a stable release.

### 10.2 Candidate asset and MSI-content verification

A fresh `msiexec /a` administrative extraction of the local candidate MSI completed with
exit **0**. The extracted package contained exactly the required named payloads:

- `coding-agent-monitor.exe`
- `ccusage.exe`
- `ccusage-antigravity.exe`
- `LICENSE`
- `THIRD_PARTY_NOTICES.md`

The extracted `LICENSE` and `THIRD_PARTY_NOTICES.md` matched the repository originals by
SHA-256. This closes the candidate **content/notices review** only; it is not a repeat of
install, launch, GUI, tray, offline, child-process, or uninstall behavior.

Candidate publication assets:

| Asset | SHA-256 |
|---|---|
| MSI | `17372F1F5634CDBD0AC9344F9321BFECCCA2C90EC8266838B733953C734BE925` |
| NSIS | `4132B4DC71793290D962D0D9B8D27C9E73004150DE4DE714C9FFF2B20192064F` |

Supporting content verification:

| File | SHA-256 | Verified scope |
|---|---|---|
| Official pinned binary, ccusage 20.0.20 | `3FD2F3B9FBA3A74881B9E75C76BAA5574F99E19142394E855C5FBAAF22F245BC` | Staged file, release sibling, and MSI-extracted `ccusage.exe` all match |
| Pinned Antigravity compatibility sidecar | `F58A76779CB938F1C954F87F757E8AE7AF8A2F1FDC241C8975CCCA377635AD42` | Staged file, release sibling, and MSI-extracted `ccusage-antigravity.exe` all match; this is a local candidate hash, not an upstream Authenticode signature or a cross-build reproducibility claim |
| `LICENSE` | `C6447A8FE0DA16AE8EF500352B442842C1F9104781C8B72C32F951AB820A6639` | Repository and MSI extraction match |
| `THIRD_PARTY_NOTICES.md` | `0221F7AE03E59581C2308BD7B78F63EE7809AFBD04D004B1CC2A38594A13E038` | Repository and MSI extraction match |

These are calculated and archived candidate checksums. They are attached, together with a
`SHA256SUMS.txt` manifest, to the draft GitHub Pre-release `v0.1.0` and are pending human
review before final publication.

### 10.3 Signing and remaining runtime scope

Windows Authenticode inspection reports `NotSigned` for the candidate MSI, NSIS installer,
main executable, official pinned ccusage binary, and pinned Antigravity compatibility
sidecar. For this pre-release the maintainer chose **unsigned** assets; no claim of
code-signing is made. The GitHub Release Notes disclose the potential SmartScreen warning
risk that unsigned installers may trigger.

The following content checks are complete: identifier confirmation, exact commit/CI identity,
candidate asset hashing, MSI administrative extraction, payload presence, both sidecar hashes,
and license/notice equality. The release-artifact install → launch → tray → offline →
child-process → uninstall cycle was rerun on 2026-08-26 against the final `com.codingagentmonitor`
artifacts and is recorded in §10.5 (no longer open).

### 10.4 Gate 0 disposition and disclosed limitation

For the v0.1.0 pre-release, the maintainer disposition is **WAIVED / NOT RUN**. A complete
human GUI cycle from a real Windows profile whose path contains non-ASCII characters was not
performed. The binary-level non-ASCII-path checks in §3 do not substitute for that GUI cycle,
so this is not a PASS. The limitation must remain disclosed in the README and GitHub Release
Notes. Phase 6–9 are already complete; Phase 10 remains a future v0.2 release-verification
task and no `V0.2_RELEASE_VERIFICATION.md` exists yet.

### 10.5 Install → launch → tray → offline → child-process → uninstall — rerun 2026-08-26

The full runtime cycle was rerun live against the final `com.codingagentmonitor` v0.1.0
artifacts (release commit `08a21c0`, CI run 32942584400). Gate 0 (non-ASCII-profile GUI) is
excluded by the maintainer waiver in §10.4; every other gate below was exercised live.

NSIS (`Coding Agent Monitor_0.1.0_x64-setup.exe`):
- **Install (silent `/S`):** exit **0**. Per-user install to
  `%LOCALAPPDATA%\Coding Agent Monitor\` containing the main exe, both sidecars, `LICENSE`,
  `THIRD_PARTY_NOTICES.md`, and `uninstall.exe`; HKCU `Uninstall` entry `DisplayVersion 0.1.0`
  present.
- **Launch / tray-resident:** started, stayed alive ≥8 s with a "Coding Agent Monitor" main
  window (tray-resident process; no crash).
- **Offline:** installed `ccusage.exe` under an empty profile returned `{"daily":[]}` with
  all-zero totals for `codex daily --json --offline` and "No usage data found." for `claude`;
  both exit **0**, no network.
- **Child-process hygiene:** after app shutdown, **zero** residual `coding-agent-monitor.exe`
  or `ccusage.exe`.
- **Uninstall (silent `/S`):** exit **0**; install dir and HKCU uninstall entry removed; zero
  residual processes.

MSI (`Coding Agent Monitor_0.1.0_x64_en-US.msi`, elevated):
- **Install (elevated, UAC approved):** exit **0**; per-user default dir
  `%LOCALAPPDATA%\Coding Agent Monitor\` with main exe, both sidecars, `LICENSE`,
  `THIRD_PARTY_NOTICES.md`, and a Start Menu `Uninstall … .lnk` shortcut; ProductCode
  `{EB418B31-DFC1-4091-9822-9611D04434EF}` registered.
- **Launch:** started and stayed alive ≥8 s with a main window (tray-resident).
- **Uninstall (elevated `/x`, UAC approved):** exit **0**; install dir and registry entry
  removed; zero residual processes.

A GitHub **draft Pre-release** `v0.1.0` (tag `v0.1.0`, marked pre-release, not yet published)
was created at `08a21c0` for human review, with the MSI, NSIS, and `SHA256SUMS.txt` attached
and the unsigned/SmartScreen and Gate 0 disclosures in the notes. Publication is intentionally
held for maintainer review.
