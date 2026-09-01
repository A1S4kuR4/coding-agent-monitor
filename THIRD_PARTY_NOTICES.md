# Third-party notices

Coding Agent Monitor is licensed under the MIT License in `LICENSE`. It uses
third-party open-source software under each component's own license. This file
ships with the Windows installers (`resources` in `src-tauri/tauri.conf.json`)
and covers everything the application actually distributes.

Since v0.3 the application does **not** bundle, download, or execute any
external `ccusage` executable. The ccusage collection sources are vendored into
the repository and compiled into the single product executable; their
provenance is machine-auditable via
`src-tauri/vendor/ccusage/UPSTREAM.toml`, `PATCHES.md`, `MANIFEST.sha256`, and
`src-tauri/vendor/ccusage/pricing/pricing-manifest.json`. The full generated
dependency license inventory (all 288 packages in the Rust dependency graph,
none blocked) is `docs/V0.3_LICENSE_INVENTORY.md`; the lock files
(`pnpm-lock.yaml`, `src-tauri/Cargo.lock`) are the authoritative version
record.

## ccusage — vendored collection engine

The Rust collection sources are vendored from ccusage release `v20.0.20`
(commit `bd7f89b469aee5635fb2e6722dd6d70f2d113ac1`, tree
`0acb7f0e9451a3094739a0caff0875ad035432e5`), under the MIT License. The
vendored copy carries its own `LICENSE` file
(`src-tauri/vendor/ccusage/LICENSE`).

Copyright (c) 2025 ryoppippi

Source: <https://github.com/ccusage/ccusage/tree/bd7f89b469aee5635fb2e6722dd6d70f2d113ac1>

The vendored sources include downstream patches maintained by this project
(see `src-tauri/vendor/ccusage/patches/` and `PATCHES.md`). They are not part
of upstream ccusage releases.

## Antigravity adapter — downstream port (not officially supported)

Support for the Antigravity agent is a port of ccusage pull request 1487
(head `c58c1b3aab2eacc82add250c8229bb6192e4489b`, tree
`c489445deffa68bcf863ed97ca29dce948c40509`, fork
`sambitcreate/ccusage`), manually maintained by the Coding Agent Monitor
maintainers onto the split-adapter v20.0.20 architecture. The pull request was
closed unmerged upstream: **Antigravity support is NOT an official ccusage
feature**, and this project makes no claim of upstream endorsement or
official support. The fork at the pinned commit retains the same MIT License.

Copyright (c) 2025 ryoppippi

Source: <https://github.com/sambitcreate/ccusage/tree/c58c1b3aab2eacc82add250c8229bb6192e4489b>

## LiteLLM pricing snapshot

An offline pricing snapshot from LiteLLM commit
`1a183efaa1a2108aed7e1bed8d445d93bd1aa60d`
(`model_prices_and_context_window.json`, SHA-256
`a74538d2edc13e1eb4f67870fbc2ee05035326e6eaed0dc5bce11d372cff6e60`) is
embedded in the product. Content outside LiteLLM's enterprise directory is
distributed under the MIT License.

Copyright (c) 2023 Berri AI

Source: <https://github.com/BerriAI/litellm/tree/1a183efaa1a2108aed7e1bed8d445d93bd1aa60d>

## models.dev pricing snapshot

The models.dev-derived pricing table shipped inside ccusage v20.0.20
(`models-dev-pricing.json`, pristine upstream SHA-256
`be347bd498cb046c2045e018e068aa228a76b34485613e2254f21a48b889eecd`; one
additive Antigravity alias entry from the patch above) is distributed under
the MIT License.

Copyright (c) 2025 models.dev

Source: <https://github.com/ccusage/ccusage/blob/bd7f89b469aee5635fb2e6722dd6d70f2d113ac1/rust/crates/ccusage-core/src/models-dev-pricing.json>

## JavaScript dependencies shipped in the product

The application bundles exactly these runtime npm packages (all MIT):

| Package | License |
| --- | --- |
| react | MIT |
| react-dom | MIT |
| @tauri-apps/api | MIT |

Development-only npm dependencies (build tooling, test runners, linters) are
not bundled into the application.

## Rust dependencies shipped in the product

The product executable statically links the Rust dependency graph recorded in
`src-tauri/Cargo.lock` (with the vendored ccusage workspace). Licenses are
MIT, Apache-2.0 (with LLVM-exception for compiler-adjacent crates), BSD-3,
ISC, MPL-2.0, Unicode-3.0, Zlib, Unlicense, and CDLA-Permissive-2.0 — all
permissive or weak-copyleft notice-type licenses compatible with binary
distribution with attribution. The generated inventory
(`docs/V0.3_LICENSE_INVENTORY.md`, 288 packages) lists every package, version,
source, license, and its per-crate copyright/notice obligations; it contains
**zero blocked packages** — no unknown, unlicensed, GPL, or AGPL components.

## Fonts, icons, and other assets

Application icons under `src-tauri/icons/` are original assets of this
project. No third-party fonts are bundled. The `LICENSE` and this notices file
are the only document resources shipped inside the installers.

## MIT License text for the components above

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Trademarks

Claude and Anthropic are trademarks of Anthropic PBC. OpenAI and Codex are
trademarks of OpenAI, L.L.C. Other names may be trademarks of their respective
owners. Coding Agent Monitor is an independent project and is not affiliated
with, endorsed by, or sponsored by those companies. Product names are used only
to describe compatibility.
