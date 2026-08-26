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
- [x] Reconfirm the exact pre-release candidate source and CI on 2026-08-26:
  local `HEAD`, `origin/main`, and GitHub `main` all resolve to
  `08a21c0177cd1ecc902584b94262d411eaf6ccaa`; [CI run 32942584400](https://github.com/A1S4kuR4/coding-agent-monitor/actions/runs/32942584400)
  passed frontend lint/typecheck/test/build (10 files / 63 tests), Rust
  fmt/strict lint, and Rust tests (37 passed / 1 ignored). No tag or GitHub
  Release exists yet.

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
  transitive crates. This is a non-blocking maintenance warning, not a known
  security vulnerability.

## Additional gates before publishing installers

- [x] Choose the final reverse-DNS Tauri identifier before the first public
  installer. The identifier is finalized as `com.codingagentmonitor`; the earlier
  `.app`-suffixed value was replaced before this candidate was built.
- [x] Record the maintainer's Gate 0 disposition for the v0.1.0 pre-release:
  **WAIVED / NOT RUN**. A complete GUI install/use/uninstall cycle from a real
  Windows profile whose path contains non-ASCII characters was not performed and
  is not a test PASS. Disclose this coverage risk in the README, verification
  record, and GitHub Release Notes.
- [x] Review the exact binary dependency notices and retain them in the bundle.
  A fresh MSI administrative extraction on 2026-08-26 exited 0 and contained the
  main executable, both sidecars, `LICENSE`, and `THIRD_PARTY_NOTICES.md`; the
  extracted license and notice files match the repository originals by SHA-256.
- [x] Confirm the signing decision for this pre-release. This v0.1.0 Pre-release
  explicitly ships **unsigned** assets: the candidate MSI, NSIS installer, main
  executable, and both sidecars report Authenticode `NotSigned`. This is not a
  signature-verification pass — no certificate is used and no claim of signing is
  made. The unsigned choice and the resulting SmartScreen / unknown-publisher
  warning are disclosed in the GitHub Release Notes.
- [x] Re-run install, launch, tray, offline, child-process, and uninstall checks
  against the final release artifacts. Performed 2026-08-26 against the
  `com.codingagentmonitor` v0.1.0 artifacts (recorded in
  `RELEASE_VERIFICATION.md §10.5`): NSIS full cycle (install → launch →
  tray-resident → offline sidecar → child-process recovery → silent uninstall)
  and MSI elevated cycle (install → launch → uninstall) both passed; zero
  residual processes after each uninstall. Gate 0 GUI remains WAIVED / NOT RUN
  per maintainer disposition.
- [x] Calculate and archive candidate-asset SHA-256 values. On 2026-08-26 the
  local v0.1.0 candidate MSI is
  `17372F1F5634CDBD0AC9344F9321BFECCCA2C90EC8266838B733953C734BE925` and the
  NSIS installer is
  `4132B4DC71793290D962D0D9B8D27C9E73004150DE4DE714C9FFF2B20192064F`.
- [ ] Publish SHA-256 checksums for the final assets attached to the GitHub
  Pre-release. `SHA256SUMS.txt` and the MSI/NSIS checksums are uploaded to the
  `v0.1.0` Draft, which remains unpublished; this item closes only when the
  Pre-release is publicly published.
- [x] Draft GitHub Pre-release notes that disclose Gate 0 as **WAIVED / NOT RUN**
  and, with the unsigned choice confirmed, the SmartScreen warning risk. Draft
  notes are written into the `v0.1.0` Pre-release and are pending maintainer
  publication.
