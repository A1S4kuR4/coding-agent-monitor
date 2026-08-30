# Antigravity golden input

The Antigravity fixture is a binary SQLite conversation database, which is
built at test time by `tests/collector_golden.rs` from the audited
generation-blob encoder (`tests/common/mod.rs`, field numbers per
`vendor/ccusage/rust/adapters/antigravity/src/parser.rs`). This directory
receives `conversations/conv-1.db` during the run and is git-ignored except
for this README.

Fixture parameters (also documented in the test):

- model: `gemini-3.1-pro-low` (vendored resolver → `gemini-3.1-pro`)
- timestamp: 2026-01-02T00:00:00Z (epoch 1_767_312_000)
- system_input 1000, fresh_input 6321 → input 7321
- cache_read 10, output 604, thinking 0, response_id `resp-1`
- expected cost: (7321×$2 + 10×$0.2 + 604×$12)/1e6 = $0.021892
