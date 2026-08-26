# ccusage sidecars

The stage script `scripts/fetch-ccusage.mjs` (run via `pnpm fetch:sidecar`)
downloads the pinned `@ccusage/ccusage-win32-x64` npm package, verifies both the
registry SHA-512 integrity and the extracted binary's SHA-256, and reproduces
`ccusage.exe` here under Tauri's target-triple filename.

- **ccusage version:** `20.0.20`
- **Source package:** `@ccusage/ccusage-win32-x64@20.0.20` (win32 / x64, no scripts)
- **Tarball URL:** `https://registry.npmjs.org/@ccusage/ccusage-win32-x64/-/ccusage-win32-x64-20.0.20.tgz`
- **Tarball SHA-512 (SRI):** `sha512-6R9cdLuRy529ewdkKbwtjx7boCQNFK8KG1F6d5G/pw2ApKRmucWcwjFo/7sNULQtZMyUxhYa40tUnWy9L/yTMA==`
- **ccusage.exe SHA-256:** `3fd2f3b9fba3a74881b9e75c76baa5574f99e19142394e855c5fbaaf22f245bc`

Expected official file: `ccusage-x86_64-pc-windows-msvc.exe`.

## Antigravity compatibility sidecar

Released ccusage 20.0.20 does not include Antigravity. The separate
`scripts/fetch-ccusage-antigravity.mjs` script therefore builds upstream PR
[#1487](https://github.com/ccusage/ccusage/pull/1487) at an exact commit and the
application invokes that binary only for `ccusage antigravity daily`. The
official 20.0.20 binary remains authoritative for the unified snapshot and all
other agents, avoiding a regression to the PR branch's older ccusage base.

- **PR commit:** `c58c1b3aab2eacc82add250c8229bb6192e4489b`
- **Embedded version:** `20.0.18-antigravity-c58c1b3`
- **Source archive SHA-512:** `f42cdf6ac8e9f375aa0cccfd97e0019166d4b331f71b8f02bb206bd4f038b7dda43b593c81488a5d0f2ae8677a32092c276f31f3308c72c87374a948648ec57e`
- **Pinned LiteLLM revision:** `34561482ed092d78c296cab7999486022af5a938`
- **Pricing snapshot SHA-512:** `0539458a2b33b3cd2eec9dc239585b98638089981fee4be8fc131c209f60b8d83a1d7cec7a7165c62b25fad220712731a68c265700cd98a7d5402077725c85a5`
- **Current local EXE SHA-256:** `f58a76779cb938f1c954f87f757e8ae7af8a2f1fdc241c8975ccca377635ad42`

Expected compatibility file:
`ccusage-antigravity-x86_64-pc-windows-msvc.exe`.

MSVC release output is not byte-for-byte stable across build paths, so the
build gate verifies the source/pricing archives, Cargo.lock, embedded version,
and Antigravity command instead of claiming a cross-machine EXE hash. The local
EXE hash above records the currently staged artifact and is not a Git-tracked
supply-chain input.

The generated binaries are intentionally excluded from Git. Run
`pnpm fetch:sidecar` after cloning; `bundle.externalBin` references the staged
files, and `src-tauri/build.rs` fails with an actionable message if either is
absent. Keeping binary artifacts out of source control avoids opaque executable
changes and makes the pinned source, registry integrity, and checksums the
reviewable supply-chain boundary.

The application only runs the sidecars from Rust as supervised, sequential
local processes. Distribution builds must retain the notices in the repository
root's `THIRD_PARTY_NOTICES.md`; Tauri bundles that file and the project license
as application resources.
