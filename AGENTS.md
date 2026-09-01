# AGENTS.md

## Tooling

- Tests (offline): `cargo test`
- Tests (real APIs): `cargo test --features integration` — network; set `FINNHUB_TOKEN` for the finnhub one
- Lint (must pass clean): `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt`

## Non-Obvious Rules
- **The CLI always runs through `/bin/sh -c 'exec "$0" "$@"'`, never direct.** A nonexistent binary handed to Quickshell 0.3.1 can abort the whole shell inside the failed start (claudebar#6), before any QML signal fires. The failed-start discriminator is `!sawExit || exitCode === 126 || exitCode === 127` on empty output; any other exited-empty run is an operational failure, never "not installed".
- **`installCmd` is the one constant** — the message shows it and the button copies it (`Util.execArgv(["wl-copy", ...])`, no shell line, no trailing newline). The button gates on `notInstalled`, never on `topError` (which also carries CLI errors). Pinned in `tests/plugin_qml.rs`.

- **Never-crash invariant:** the binary MUST always exit 0 with valid JSON (waybar or `--output json` structured — invalid argv is pre-scanned so the fallback speaks the requested format), even on failure. No top-level `unwrap`/`expect`; `main` uses `try_parse` + `catch_unwind`. `--help`/`--version` plus the manual `--preset`/`--list-presets` helpers are the only deliberate non-JSON exits (never invoked by a bar).
- `FINNHUB_TOKEN` is sent via the `X-Finnhub-Token` header, never in a URL — keep it out of any URL/error string/log.
- **Monochrome is a palette, not a branch:** `--no-color` works by handing a surface `ThemeColors::monochrome()` (all fields empty), and `waybar::fg`/`bold_fg` emit no `foreground=` for an empty color. So EVERY tint must come from a `ThemeColors` field and go through those two helpers — never `format!("<span foreground=…")` inline, or that surface will keep its color in monochrome. `render::build` picks the palette per surface; `class` and the structured JSON carry no color and are untouched.
- **The core owns the colours; frontends never re-derive them.** `render::direction_color` is the single source of truth for up/down/flat, and `data::Palette::from_theme` publishes it (plus `text`/`dim`/`accent`/`error`) as the structured document's `palette`. The QML panel and bar strip consume that; they must NOT blend their own tint from `Color.accent`/`urgent` (doing so is what made `+2.14%` render accent-blue in the panel and theme-green in the bar). The published palette is never monochrome — `--no-color` only affects the Pango surfaces.
- The tooltip ALWAYS uses the Nerd icon set regardless of the configured `icons` (consistent monospace column alignment). Column widths are measured with `waybar::visible_len` (excludes Pango tags); the aligned change column intentionally carries no glyph (Nerd glyph widths are unreliable in measured columns).
- Cache keys embed `cache::SCHEMA_VERSION`; bump it when the cached `Quote`/`Record` format changes.
- Price `0`/null/NaN/empty ⇒ Missing; but `change` of `0.0` is valid (Flat).

## Project-Specific Patterns

- **`screenshots/demo/demo-data` RE-IMPLEMENTS the renderer in bash.** It emits the whole Waybar document — bar strip, tooltip, structured JSON — from a fixed watchlist instead of calling the binary. The README screenshots come from it, so a change to the tooltip's shape has to be mirrored there in the same commit, or the published screenshots stop showing the product. `MAX_COLUMNS`/`ROWS_PER_COLUMN` near the top of the file decide the banding the screenshots show.

- Vertical Slice Architecture: each data source is a self-contained slice in `src/providers/`; cross-cutting code is in `src/platform/`. There is **no `Provider` trait** — `providers::mod` dispatches via a `match` on `ProviderKind`.
- Adding a provider = new `src/providers/<name>.rs` (with a pure `parse`/`parse_one` + `#[cfg(test)]` tests) + a `ProviderKind` variant + arms in `providers::{ttl, fetch_kind}`, `platform::icons::kind_glyph`, `platform::render::group_of` (maps the source to a `TooltipGroup`), and `platform::market::spec` (market calendar, or `None` for 24/7). The tooltip groups by `TooltipGroup` derived from `cfg.assets`, not by `ProviderKind`.
- Caching lives only in `platform::cache::get_or_fetch(key, ttl, now, fetch_fn)`; slices never cache. Test error/backoff paths by faking the `fetch_fn` closure; test provider HTTP wiring with `mockito` via `Http::with_base_url`; test parsers with fixtures in `tests/fixtures/`.
- Tests follow behavior-focused names (no method names); see existing `#[test]` fns.

