# Open-source release checklist

This checklist separates publishing the source repository from distributing a
Windows installer. The source can be published before every installer gate is
closed, provided the repository is clearly marked pre-release.

**Current release:** [v0.5.0](https://github.com/A1S4kuR4/coding-agent-monitor/releases/tag/v0.5.0),
published 2026-09-23. The frozen installers, disclosed coverage gaps and
remote asset verification are recorded in
[the verification record](V0.5_RELEASE_VERIFICATION.md). The previous public
release was [v0.3.0](https://github.com/A1S4kuR4/coding-agent-monitor/releases/tag/v0.3.0).
The preparation and v0.1/v0.2
sections below retain dated evidence (including old sidecar packaging and audit
counts). They are not current setup instructions or fresh security scans. Use
[CONTRIBUTING.md](../CONTRIBUTING.md) for development and the
[v0.3 gate decision](V0.3_RELEASE_GATE_DECISION.md) for current support/waivers.

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
  Release existed at that check; subsequent publication is recorded below.

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
- [x] Publish SHA-256 checksums for the final assets attached to the GitHub
  Pre-release. `SHA256SUMS.txt` and the MSI/NSIS checksums were uploaded to the
  `v0.1.0` Draft and are now publicly published (2026-08-26). The published
  Pre-release carries tag `untagged-0d7335725533fc748ffe` at `08a21c0…`.
- [x] Draft GitHub Pre-release notes that disclose Gate 0 as **WAIVED / NOT RUN**
  and, with the unsigned choice confirmed, the SmartScreen warning risk. Draft
  notes were written before publication; the Pre-release was subsequently
  published as recorded in the preceding item.

## v0.2.0 release record (2026-08-29)

- [x] Phase 10 verification completed and recorded in
  `docs/V0.2_RELEASE_VERIFICATION.md` (2026-08-28): automated suite, real-data
  smoke, NSIS + MSI install→launch→uninstall cycles, manual GUI walkthrough on
  real data. Gate 0 (non-ASCII user-profile GUI) again **WAIVED / NOT RUN** by
  maintainer disposition; 200% text-scaling walkthrough NOT RUN (WebView2 does
  not respond to synthesized zoom input in this app).
- [x] Mark v0.2.0 as release candidate per the v0.2 plan §4 acceptance criteria
  (`docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md` §13). Verification ran on
  commit `6bc54f5`; RC installers were built from `4d2e3a9` (docs + version
  bump only in between).
- [x] Merge the release branch to `main` via
  [PR #13](https://github.com/A1S4kuR4/coding-agent-monitor/pull/13); `main` at
  merge commit `d18c02c`.
- [x] Build the release installers from `d18c02c`: MSI
  `30C8C890E0470A62D02405680D8FA5982CD15F4B4C5A7857F76343AEF9F352A2`, NSIS
  `3BB0C9564D3E5DEAD9617B2B952D47659A817EB138173068C19EBC590A732FE1`. Assets
  remain unsigned (Authenticode `NotSigned`), disclosed in the release notes.
- [x] Publish `SHA256SUMS.txt` and both installers with the public `v0.2.0`
  GitHub Release (published 2026-08-29). The release notes disclose the Gate 0
  waiver and the SmartScreen / unknown-publisher risk, and point to
  `V0.2_RELEASE_VERIFICATION.md`.

## v0.3.0 release record (published 2026-09-01; checked 2026-09-06)

- [x] Replace the external sidecar supply chain with vendored ccusage and the
  same-product-EXE worker. See [Phase 5](V0.3_PHASE5_RELEASE_CANDIDATE.md) for
  cleanup, installer inspection, and actual verification outcomes.
- [x] Freeze release gates and record the maintainer's three waivers: unsigned
  assets, Windows 10 lifecycle unverified (official support narrowed to Windows
  11 x64), and non-ASCII-profile GUI unverified. See
  [the final decision](V0.3_RELEASE_GATE_DECISION.md); waivers are not test passes.
- [x] Publish the non-draft, non-prerelease `v0.3.0` GitHub Release at
  `2026-09-01T03:58:08Z`, with MSI, NSIS, and `SHA256SUMS.txt` assets.
- [x] Correct checksum filenames to match GitHub download names (dots in place
  of spaces), commit `edecd25`. Both published installer digests match the
  repository's checksum entries; the published checksum-file digest matches
  the local file hash. Exact names, sizes, and hashes are in
  [the publication result](V0.3_RELEASE_GATE_DECISION.md#6-实际发布结果2026-09-06-只读复核).

The 2026-09-06 check inspected Release metadata and the local checksum file. It
did not rebuild, reinstall, or re-run the historical acceptance/security suite.

## v0.5.0 release record (published 2026-09-23)

- [x] Freeze the binary build-source commit at `2927799940b9d37e4e84707e0b4837368fe00add`;
  later release-document edits do not affect installer bytes.
- [x] Pass current-host frontend, Cargo, vendored-source, native display and
  installed-package lifecycle checks, including the final NSIS tray menu and
  preference-failure paths. Keep first-attempt failures and proxy limits in
  [the verification record](V0.5_RELEASE_VERIFICATION.md).
- [x] Record the maintainer's instruction to skip the developer-tools-free
  offline clean host and real logon/reboot plus sleep/resume checks. The full
  non-ASCII-profile GUI scenario also remains unrun. Release notes disclose
  these gaps and the unsigned package status; none is labeled PASS.
- [x] Freeze installer SHA-256 values: MSI
  `f2f9751069053cefd679046e2d99576217f2c2de327c4b348e8e72b4e394c333`,
  NSIS `874f7616531bdbfff1952f0f6f71cab16ba7d2e4f1d58ae5bf45224cdb99d88a`.
  Publication filenames and checksum entries use dots in place of spaces,
  matching the v0.3 GitHub asset convention; binary bytes are unchanged.
- [x] Review the open moderate `glib` lockfile advisory. The Windows x64 target
  dependency tree excludes it; the all-target graph reaches it through GTK and
  WebKit. The alert remains open for non-Windows maintenance and is not counted
  as a Windows installer vulnerability. Production npm audit on the official
  registry found zero advisories.
- [x] Merge [PR #24](https://github.com/A1S4kuR4/coding-agent-monitor/pull/24)
  to `main` at `efde53bcd391ae93a647b86bce1dbec9c2a838bc` after its CI
  passed; the independent `main` push CI also passed. Push annotated `v0.5.0`
  to that merge commit.
- [x] Publish the non-draft, non-prerelease
  [v0.5.0 GitHub Release](https://github.com/A1S4kuR4/coding-agent-monitor/releases/tag/v0.5.0)
  at `2026-09-23T11:49:38Z` with MSI, NSIS and `SHA256SUMS.txt`. All three
  GitHub asset digests match re-downloaded bytes; the checksum file verifies
  both installer downloads. Exact names, sizes and hashes are in the
  [publication result](V0.5_RELEASE_VERIFICATION.md#public-release-result-2026-09-23).
