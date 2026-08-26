# Contributing

Thanks for helping improve Coding Agent Monitor.

## Before opening a change

- Keep the application Windows-focused, local-first, and lightweight.
- Never commit real coding-agent logs, token exports, costs, credentials, user
  profile paths, or signing certificates.
- Keep frontend code independent of raw ccusage JSON; normalization belongs at
  the Rust sidecar boundary.
- For behavior changes, add focused TypeScript or Rust tests.

The detailed architecture and scope constraints are in `AGENTS.md` and
`docs/IMPLEMENTATION_PLAN.md`.

## Development setup

Requirements: Windows 10/11 x64, Node.js 20 or newer, pnpm 10.33.0, Rust stable,
and the Tauri 2 Windows prerequisites.

```powershell
pnpm install --frozen-lockfile
pnpm fetch:sidecar
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

## Pull requests

Explain the problem, the chosen solution, and the checks actually run. Keep
unrelated refactors out of the same pull request. UI changes should include a
synthetic-data screenshot only; do not capture personal usage.

By contributing, you agree that your contribution is licensed under the
project's MIT License.
