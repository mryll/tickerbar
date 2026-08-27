# AGENTS.md

## Tooling

- Tests (offline): `cargo test`
- Tests (real APIs): `cargo test --features integration` — network; set `FINNHUB_TOKEN` for the finnhub one
- Lint (must pass clean): `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt`

## Non-Obvious Rules
- **Quickshell emits NEITHER `started` NOR `exited` when the command does not exist** — `running` just drops back to false. That is the only signal a failed start gives. Anything that waits on `onExited` to leave a loading state hangs for ever when the CLI is not installed, which is the first run of everyone who installs the plugin from the marketplace: the plugin is a git clone, the CLI is a package, and nothing installs the second for you. The `onRunningChanged` guard in the panel's `Process` is what makes the not-installed message reachable — verified against a running shell, not assumed.

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

## Release

A release is automated by pushing a tag — do NOT build or upload the binary by hand:

1. Bump `version` in `Cargo.toml` + `Cargo.lock` AND in `manifest.json` (the marketplace shows the manifest's version; it must equal the tag); commit `chore: release X.Y.Z` on `develop` and push.
2. Move master to the release — master only advances here: `git push origin develop:master`. Then `git tag vX.Y.Z && git push origin --tags`.
3. The tag push triggers `.github/workflows/release.yml`, which builds and publishes the GitHub release with the asset `tickerbar-X.Y.Z-x86_64-linux` (consumed by the `tickerbar-bin` AUR package).
4. Only after the release exists, bump both AUR repos (`aur/tickerbar` source + `aur/tickerbar-bin`) per the workspace `AGENTS.md`. Order matters: `updpkgsums` fetches the tag tarball AND the release asset, so both must already be live.
