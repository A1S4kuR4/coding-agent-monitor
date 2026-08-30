# Downstream patches over vendored ccusage v20.0.20

Every deviation of the vendored tree
(`rust/` + `pricing/` + `UPSTREAM.toml` + this manifest) from pristine upstream
ccusage **v20.0.20** (`bd7f89b469aee5635fb2e6722dd6d70f2d113ac1`) is recorded
here. `rust/` is otherwise a byte-faithful, LF-normalized subset of the
upstream `rust/` workspace (scope filter in [UPSTREAM.toml](UPSTREAM.toml)).

Verification state at the time of writing: `cargo test --workspace` in
`rust/` passes — 482 passed, 0 failed (2 ignored), including all upstream
adapter/pricing tests.

---

## 0001 — Antigravity adapter port (PR #1487)

- **Source**: closed, unmerged downstream patch
  https://github.com/ccusage/ccusage/pull/1487 — fork
  `sambitcreate/ccusage` commit `c58c1b3aab2eacc82add250c8229bb6192e4489b`,
  based on ccusage `739e88fa67b9e584dfa9722c8207fa8b09b62802`
  (v20.0.18-era **monolithic** `rust/crates/ccusage` crate). Original patch
  file kept verbatim at `patches/0001-antigravity-c58c1b3.patch` for audit;
  the vendored tree is a **manual port**, not a `git apply` of that file.
- **License**: MIT (same as upstream).

### Files involved

| Path | Change |
| --- | --- |
| `rust/adapters/antigravity/` | **new crate** (from PR `rust/crates/ccusage/src/adapter/antigravity/{loader,parser,paths,proto,report}.rs` + `mod.rs` → `lib.rs`) |
| `rust/Cargo.toml` | workspace member via `adapters/*` glob + `ccusage-adapter-antigravity` in `[workspace.dependencies]` |
| `rust/Cargo.lock` | regenerated (new crate; entries for removed scope crates absent, see 0002) |
| `rust/crates/ccusage-core/src/lib.rs` | `"antigravity"` appended to `BUILT_IN_AGENT_NAMES` |
| `rust/crates/ccusage-adapter-all/Cargo.toml` | `ccusage-adapter-antigravity.workspace = true` |
| `rust/crates/ccusage-adapter-all/src/lib.rs` | re-export `pub use ccusage_adapter_antigravity as antigravity;` |
| `rust/crates/ccusage-adapter-all/src/loader.rs` | `AgentLoadSpec` registration (index 16) wiring `antigravity::load_entries` / `summarize_entries` |
| `rust/crates/ccusage-adapter-all/src/report.rs` | agent label `"antigravity" => "Antigravity"` |
| `rust/crates/ccusage-adapter-all/src/tests.rs` | `ANTIGRAVITY_DATA_DIR` added to the per-agent env-var table |
| `rust/crates/ccusage-core/src/models-dev-pricing.json` | pricing snapshot, see "Pricing snapshot strategy" below |

### Semantic differences vs the original patch

The PR targeted the v20.0.18 single-crate layout; v20.0.20 split it into
`ccusage-core` + `adapters/*`. The port therefore:

1. **Module → crate.** `adapter/antigravity/mod.rs` becomes a standalone
   `adapters/antigravity` crate (`mod.rs` → `lib.rs`, own `Cargo.toml`).
   `pub(crate)` items used across what are now crate boundaries
   (`load_entries`, `summarize_entries`, `report_from_rows`,
   `summarize_entries`) were widened to `pub`; pure-internal items stayed
   `pub(crate)`.
2. **Imports/visibility rewritten to the split architecture.**
   `crate::{…}` monolith paths became explicit external imports:
   - `ccusage_core::{LoadedEntry, PricingMap, Result, debug_log, parse_tz, …}`
   - `ccusage_core::progress::{UsageLoadAgent, track_usage_load}` — the PR
     added an `Antigravity` variant to the monolith's `progress` enum; in the
     split layout the adapter passes `UsageLoadAgent("Antigravity")` as a
     newtype value instead (no core enum change needed).
   - `ccusage_adapter_common::{read_files_parallel, collect_files_with_extension}`
   - `ccusage_cli::{CostMode, SharedArgs}`
3. **`report.rs` reuses core's `agent_summary_json`.** The PR formatted agent
   rows by borrowing the opencode module's helper across the monolith
   (`super::super::opencode::agent_summary_json`). v20.0.20 moved that helper
   to `ccusage_core::agent_summary_json`; the port imports it from core, so
   the JSON row shape is produced by the shared core implementation exactly
   like every other adapter.
4. **Pricing snapshot: additive merge instead of wholesale replacement.**
   The PR replaced `models-dev-pricing.json` wholesale with a 448-entry
   Gemini-extended snapshot built on the older models.dev catalog. Applied
   verbatim on v20.0.20 that replacement **drops 1826 upstream entries** —
   among them the `gpt-5.6*` family with its long-context tier pricing,
   which broke the upstream test
   `ccusage_adapter_codex::tests::prices_gpt_5_6_long_context_usage_from_embedded_pricing`.
   The vendored tree instead keeps the **pristine v20.0.20 snapshot**
   (2274 entries, upstream blob SHA-256
   `be347bd498cb046c2045e018e068aa228a76b34485613e2254f21a48b889eecd`) and
   additively merges the single entry the Antigravity resolver needs and
   upstream lacks:
   - `gemini-3.1-pro` — `cost: {cache_read: 0.2, cache_write: 0.375,
     input: 2, output: 12}` ($/M), `limit.context: 1048576`; values taken
     unchanged from the PR snapshot.

   Final vendored file: 2275 entries, SHA-256
   `719203dac7ed169af7288449eb6dc0b89026fa4ebdb609b27eeb0c5adf98e528`
   (also recorded in `pricing/pricing-manifest.json`).

### Antigravity models intentionally left unpriced

`adapters/antigravity/src/parser.rs::resolve_model_name` maps raw Antigravity
model ids to LiteLLM-priced names. The following resolved names exist in
**neither** the upstream v20.0.20 snapshot nor the PR #1487 snapshot, so they
have no pricing data:

- `gemini-3.5-flash-high`
- `gemini-3.5-flash-medium`
- `gemini-3.5-flash-extra-low`
- `gpt-oss-120b-medium`
- `gemini-3-flash-a`, `gemini-3-flash-b`, `gemini-3-flash-c`
  (resolved targets `gemini-3.5-flash-high` / pass-through raw ids)

Per the product contract, missing pricing yields a **null cost** — we do not
fabricate `$0`, and no synthetic entries are added for these. If models.dev
publishes them later, updating `models-dev-pricing.json` and the SHA-256 in
`pricing/pricing-manifest.json` is the only change required.

---

## 0002 — Coding Agent Monitor (CAM) adaptations

Downstream-only changes with no upstream counterpart, required to consume the
vendored workspace as an in-process library inside this app.

### `rust/crates/ccusage-core/build.rs` — vendored pricing fallback

Upstream's build script fetches the LiteLLM pricing snapshot from the network
(`fetch_pricing_json()` driven by `flake.lock`) when the `fetch` feature path
is taken. CAM builds must work **offline and never panic on a missing env
var**, so the build script now prefers a vendored copy: it reads
`<vendor-root>/pricing/litellm-pricing.json` (pinned, SHA-256 recorded in
`pricing/pricing-manifest.json`) and only falls back to the upstream network
fetch if that file is absent. The `fetch` feature stays off for the product.

### `rust/crates/ccusage-adapter-all/src/lib.rs` — in-process collector entry point

New public function `daily_report_json_by_agent(&SharedArgs) -> Result<Value>`:
runs the unified loader (`load_rows(AgentReportKind::Daily, …)`) and returns
the same JSON shape `ccusage daily --json --by-agent` prints, without going
through the CLI parser, terminal rendering, or stdout. Callers set
`json: true`, `offline: true` (plus `timezone: Some("UTC")`) on `SharedArgs`.
`SharedArgs` / `AgentReportKind` imports were widened accordingly. This is
the seam the Gate 0 PoC test in `src-tauri/` exercises; the production
sidecar collection path is **not** switched to it by this patch series.

### `rust/crates/ccusage-core/src/types.rs` — serialize `missingPricing` on model breakdowns

Upstream keeps `ModelBreakdown::missing_pricing` behind
`#[serde(skip_serializing)]`, so a model with no pricing entry is
indistinguishable from a model priced at exactly $0 in the unified JSON
(both render `"cost": 0.0`). That breaks the CAM null-vs-zero cost contract.
The patch removes the skip attribute so each model breakdown serializes
`"missingPricing": true|false` (purely additive field). Downstream tests
updated accordingly (copilot/qwen inline JSON expectations, core insta
snapshots) — those hunks are part of this patch file.

### `rust/crates/ccusage-core/src/load_context.rs` + per-agent loader — structured in-process collection (Phase 1)

New core module `load_context`: load-scoped stores (process-global, cleared at
the start and drained at the end of every load; loads serialize behind a
mutex and run single-threaded) for:

- `LoadDiag { agent, kind, file, details }` — recoverable problems observed
  while loading (corrupt file, skipped record, SQLite open/query failure,
  unreadable source);
- `LoadFailure { kind, details }` — fatal failures with a machine-readable
  `LoadFailureKind` (`SourceUnavailable`, `InvalidConfig`, `Database`,
  `Internal`), so callers classify without matching error text;
- a per-load data-root override consulted by every adapter's path resolver
  before environment/default resolution.

`ccusage-adapter-all` gains `AgentLoadOutcome` and
`daily_report_for_agent(agent, root_override, shared)`: executes exactly one
agent's load spec (other agents' loaders are never constructed or run), maps
failures structurally, and returns diagnostics alongside the report.
`daily_report_json_by_agent` (the all-agents Gate 0 seam) is unchanged.

Consequences across adapters (all part of this patch file):

- every adapter's path resolver short-circuits to the explicit override when
  one is installed for its agent (`claude`, `codex` interpreted as CODEX_HOME
  homes, the rest as their env-var-equivalent roots);
- the claude root-validation failure raises a structured `SourceUnavailable`
  before returning the existing `CliError` (message unchanged for CLI users);
- "Failed …" skip sites in every adapter's loader additionally record a
  `LoadDiag` (kind `CorruptFile` or `DatabaseError`);
- the antigravity `gen_metadata` prepare-failure branch (previously silent)
  records a `DatabaseError` diagnostic;
- loader: `load_rows_filtered` executes a filtered spec list; unknown agent
  → `Failed(InvalidConfig)`.

### `rust/crates/ccusage-adapter-all/src/tests.rs` — per-agent loader tests

Five new tests (isolation from invalid other-agent roots, override beats
environment, structural claude failure, corrupt-database diagnostic,
unknown-agent failure) plus a local single-acquisition env guard — the
shared `isolated_agent_env` helper is unchanged.

### Import-scope filtering (part of the import, not upstream code)

Per `UPSTREAM.toml [import] scope`, the vendored workspace excludes the CLI
binary surface and non-essentials: `crates/ccusage-cli-parser`,
`crates/ccusage-config`, npm launcher, docs site, benchmarks. Consequences
visible in `rust/Cargo.toml` / `Cargo.lock`:
`ccusage-cli-parser`, `ccusage-config`, `mimalloc`, `schemars`, `ureq`
dropped from `[workspace.dependencies]`; the `ccusage-cli` library crate is
kept (adapters need `SharedArgs`/`CostMode`). `adapters/AGENTS.md` and
`adapters/CLAUDE.md` (upstream agent-guidance docs) are also excluded.

### Test-only assertion fix

`ccusage-core/src/pricing.rs` embedded-pricing test: upstream asserted
`pricing.find("claude-opus-5").is_none()`, but the v20.0.20 embedded
models.dev snapshot *does* price `claude-opus-5` (the upstream assertion was
written against an older snapshot). The vendored tree asserts
`pricing.find("claude-opus-6").is_none()` instead — same intent
(a model no source prices yet), satisfied by the actual pinned snapshot.

---

## Regression risk & upgrade path

- `scripts/vendor-ccusage-import.mjs` rebuilds the vendor tree from the pinned
  commits and **automatically re-applies this patch series** as
  `patches/0002-cam-downstream-v20.0.20.patch` (the apply-able representation
  of every committed downstream edit; 0001 is the verbatim upstream PR diff,
  kept as an audit reference only — it does not apply to v20.0.20). Before
  swapping, the rebuild is byte-compared against the committed vendor blobs;
  any drift aborts the import. Direct vendor edits must therefore be folded
  back into 0002 (see the script header for the regeneration flow) or the
  next rebuild will fail by design. `pnpm vendor:verify` re-checks the
  committed state offline at any time.
- All changes above are confined to `rust/crates/ccusage-core` +
  `rust/crates/ccusage-adapter-all` + the new `rust/adapters/antigravity`
  crate; no adapter behavior other than antigravity registration is touched.

### Line-level skip diagnostics + path sanitization (claude daily/session readers, antigravity)

- `adapters/claude/src/daily.rs` (the Daily aggregation reader) records a
  `CorruptRecord` diagnostic for skipped usage records (unsupported null
  fields, invalid JSON, unparseable timestamp) — previously fully silent.
- `adapters/claude/src/lib.rs` (session-path reader) gets the same wiring for
  parity between report kinds.
- Diagnostics record only the source file NAME in the structured `file` field
  (never the full local path), keeping error/diagnostic output free of
  unnecessary user paths.
- `adapters/antigravity/src/loader.rs` gen_metadata prepare-failure branch is
  likewise sanitized (file name only in the structured field).
