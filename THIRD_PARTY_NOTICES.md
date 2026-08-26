# Third-party notices

Coding Agent Monitor is licensed under the MIT License in `LICENSE`. It uses
third-party open-source software under each component's own license. The lock
files (`pnpm-lock.yaml` and `src-tauri/Cargo.lock`) are the authoritative record
of dependency versions.

## ccusage sidecars

Windows distributions may contain two `ccusage` executables:

- `ccusage` 20.0.20, obtained from `@ccusage/ccusage-win32-x64@20.0.20`.
  Source: <https://github.com/ccusage/ccusage/tree/v20.0.20>
- An Antigravity compatibility build from ccusage pull request 1487, pinned to
  commit `c58c1b3aab2eacc82add250c8229bb6192e4489b`.
  Source: <https://github.com/sambitcreate/ccusage/tree/c58c1b3aab2eacc82add250c8229bb6192e4489b>

Both are derived from ccusage and distributed under the MIT License.

Copyright (c) 2025 ryoppippi

## LiteLLM pricing snapshot

The compatibility sidecar build embeds a pricing snapshot from LiteLLM commit
`34561482ed092d78c296cab7999486022af5a938`.
Source: <https://github.com/BerriAI/litellm/tree/34561482ed092d78c296cab7999486022af5a938>

Content outside LiteLLM's enterprise directory is distributed under the MIT
License.

Copyright (c) 2023 Berri AI

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

## Other dependencies

JavaScript and Rust dependencies include MIT, Apache-2.0, BSD, ISC, MPL-2.0,
Unicode, Zlib, and similarly permissive or weak-copyleft components. Before
publishing a binary release, regenerate and review the dependency license list:

```powershell
pnpm licenses list
cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1
```

Development-only dependencies are not bundled into the application merely by
being present in `pnpm-lock.yaml`. Binary distributors remain responsible for
retaining every notice required by the exact artifacts they distribute.

## Trademarks

Claude and Anthropic are trademarks of Anthropic PBC. OpenAI and Codex are
trademarks of OpenAI, L.L.C. Other names may be trademarks of their respective
owners. Coding Agent Monitor is an independent project and is not affiliated
with, endorsed by, or sponsored by those companies. Product names are used only
to describe compatibility.
