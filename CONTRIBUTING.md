# Contributing

Thanks for helping improve Coding Agent Monitor.

## Before opening a change

- Keep the application Windows-focused, local-first, and lightweight.
- Never commit real coding-agent logs, token exports, costs, credentials, user
  profile paths, or signing certificates.
- Keep frontend code independent of raw ccusage JSON and vendored types.
  `collector/ccusage.rs` converts vendor output to CAM types;
  `sidecar/adapter.rs::normalize_snapshot` produces the public `UsageSummary`.
- For behavior changes, add focused TypeScript or Rust tests.

The detailed architecture and scope constraints are in `AGENTS.md` and
`docs/IMPLEMENTATION_PLAN.md`.

## Development setup

Requirements: Windows 11 x64, Node.js 20 or newer, pnpm 10.33.0, Rust stable,
and the Tauri 2 Windows prerequisites (MSVC build tools and WebView2).
Windows 10 and full GUI use under a non-ASCII Windows profile are unverified;
see the [v0.3 support decisions](docs/V0.3_RELEASE_GATE_DECISION.md).

```powershell
pnpm install --frozen-lockfile
pnpm vendor:verify
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

Launch the desktop app with `pnpm tauri dev`; `pnpm dev` alone serves the
frontend and does not provide the native collection command. Build installers
with `pnpm tauri build`. For UI changes, also run `pnpm test:e2e` (use
`pnpm test:e2e --workers=2` when memory is limited). The checked-in Playwright
configuration uses the installed Microsoft Edge browser (`channel: "msedge"`)
and a synthetic-data IPC harness; it does not replace native installer/GUI
acceptance.

Collection sources and pricing are checked in under `src-tauri/vendor/ccusage/`.
Normal builds do not fetch ccusage executables, source snapshots, or prices;
initial npm/Cargo dependency installation can still require network access.
The product starts its own EXE as a hidden worker. There is no `fetch:sidecar`
setup step. Upstream or pricing upgrades must follow the
[v0.3 upgrade procedure](docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md#11-上游升级流程)
and preserve the vendor manifests and replayable patches.

`src-tauri/tests/shadow17.rs` and `sidecar_shadow.rs` are optional upgrade audits.
They require externally pinned binaries supplied through
`CAM_SHADOW_SIDECAR_EXE` and `CAM_SHADOW_ANTIGRAVITY_EXE`; without them, the
sidecar comparisons skip. A default test pass does not certify fresh shadow
parity. `scripts/export-token-usage.mjs` is a legacy v0.2 helper that still
requires the removed sidecar layout; it is not a v0.3 export command.

## Pull requests

Explain the problem, the chosen solution, and the checks actually run. Keep
unrelated refactors out of the same pull request. UI changes should include a
synthetic-data screenshot only; do not capture personal usage.

By contributing, you agree that your contribution is licensed under the
project's MIT License.
