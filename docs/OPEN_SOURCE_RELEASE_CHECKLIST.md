# Open-source release checklist

This checklist separates publishing the source repository from distributing a
Windows installer. The source can be published before every installer gate is
closed, provided the repository is clearly marked pre-release.

## Completed repository preparation

- [x] Project license added and package manifests declare MIT.
- [x] Third-party sidecars and LiteLLM pricing data attributed.
- [x] Real usage exports, data-backed prototypes, logs, build output,
  screenshots, certificates, and generated sidecar executables excluded from Git.
- [x] Personal usernames, machine account inventory, and absolute workstation
  paths removed from public-facing verification records.
- [x] Security and contribution policies added.
- [x] Original application icon replaces the Tauri starter icon.
- [x] CI and dependency update configuration added.
- [x] Source-tree secret and personal-data scan completed with no credential hit.
- [x] Production npm dependency audit completed against the public npm registry
  with no known vulnerability reported on 2026-08-26.
- [x] RustSec scanned all 438 packages in `Cargo.lock` on 2026-08-26 and reported
  no vulnerability advisory. It reported 17 non-vulnerability warnings: 12 are
  outside the Windows target graph; five unmaintained `unic-*` crates are
  transitive through Tauri's `urlpattern` dependency.

## Before making the GitHub repository public

- [x] Review `git diff --cached` and the complete first commit. Commit `8c5da5f`
  "Initial open-source release of Coding Agent Monitor (v0.1.0 pre-release)",
  reviewed on 2026-08-26.
- [x] Confirm the GitHub repository description, topics, and owner are correct.
  `A1S4kuR4/coding-agent-monitor` (PUBLIC): description and six topics
  (coding-agent, token-usage, monitoring, tauri, rust, windows) verified via `gh`.
- [x] Enable Private vulnerability reporting in GitHub Security settings. Verified
  `{"enabled":true}` on 2026-08-26.
- [x] Enable branch protection after the default branch exists. `main` protected:
  1 required approval and strict status checks on `CI / validate` for
  non-administrators; `enforce_admins` is `false`, so the repository owner
  (administrator) can bypass the review/status checks and push or merge directly
  — an intentional configuration for a single-maintainer repository.
- [x] Verify that `token-usage-last7days.json`, `tauri-dev.log`, `screenshots/`,
  `src-tauri/target/`, and both sidecar EXEs are absent from the commit.
- [x] Run the validation commands in `CONTRIBUTING.md` once more from the exact
  commit that will be pushed. All seven gates pass from `8c5da5f` on 2026-08-26:
  pnpm lint / typecheck / test (10 files, 63 tests) / build, cargo fmt --check /
  clippy -D warnings / test (37 passed). No files changed, no amend required.
- [ ] Monitor Tauri / `urlpattern` for removal of the five unmaintained `unic-*`
  transitive crates. This is a maintenance warning, not a known vulnerability.

## Additional gates before publishing installers

- [x] Choose the final reverse-DNS Tauri identifier before the first public
  installer. The current `com.codingagentmonitor.app` works on Windows but Tauri
  warns about the `.app` suffix; changing identity after release can disrupt
  upgrades or per-user data paths. The identifier was finalized as
  `com.codingagentmonitor` (dropping the `.app` suffix).
- [ ] Complete the still-open full-GUI test from a real Windows profile whose
  path contains non-ASCII characters (Gate 0 in `RELEASE_VERIFICATION.md`).
- [ ] Review the exact binary dependency notices and retain them in the bundle.
- [ ] Decide whether to code-sign the Windows installer. Unsigned binaries may
  trigger SmartScreen warnings; do not claim they are signed.
- [ ] Re-run install, launch, tray, offline, child-process, and uninstall checks
  against the final release artifacts.
- [ ] Publish SHA-256 checksums for every release asset.
