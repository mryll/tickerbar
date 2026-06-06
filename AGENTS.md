# AGENTS.md

## Tooling

- Tests (offline): `cargo test`
- Tests (real APIs): `cargo test --features integration` — network; set `FINNHUB_TOKEN` for the finnhub one
- Lint (must pass clean): `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt`

## Non-Obvious Rules

- **Never-crash invariant:** the binary MUST always exit 0 with valid Waybar JSON, even on failure. No top-level `unwrap`/`expect`; `main` uses `try_parse` + `catch_unwind`. `--help`/`--version` are the only deliberate non-JSON exits.
- `FINNHUB_TOKEN` is sent via the `X-Finnhub-Token` header, never in a URL — keep it out of any URL/error string/log.
- The tooltip ALWAYS uses the Nerd icon set regardless of the configured `icons` (consistent monospace column alignment). Column widths are measured with `waybar::visible_len` (excludes Pango tags); the aligned change column intentionally carries no glyph (Nerd glyph widths are unreliable in measured columns).
- Cache keys embed `cache::SCHEMA_VERSION`; bump it when the cached `Quote`/`Record` format changes.
- Price `0`/null/NaN/empty ⇒ Missing; but `change` of `0.0` is valid (Flat).

## Project-Specific Patterns

- Vertical Slice Architecture: each data source is a self-contained slice in `src/providers/`; cross-cutting code is in `src/platform/`. There is **no `Provider` trait** — `providers::mod` dispatches via a `match` on `ProviderKind`.
- Adding a provider = new `src/providers/<name>.rs` (with a pure `parse`/`parse_one` + `#[cfg(test)]` tests) + a `ProviderKind` variant + arms in `providers::{ttl, fetch_kind}`, `platform::icons::kind_glyph`, `platform::render::group_of` (maps the source to a `TooltipGroup`), and `platform::market::spec` (market calendar, or `None` for 24/7). The tooltip groups by `TooltipGroup` derived from `cfg.assets`, not by `ProviderKind`.
- Caching lives only in `platform::cache::get_or_fetch(key, ttl, now, fetch_fn)`; slices never cache. Test error/backoff paths by faking the `fetch_fn` closure; test provider HTTP wiring with `mockito` via `Http::with_base_url`; test parsers with fixtures in `tests/fixtures/`.
- Tests follow behavior-focused names (no method names); see existing `#[test]` fns.
