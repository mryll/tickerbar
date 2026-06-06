# tickerbar

[![AUR version](https://img.shields.io/aur/version/tickerbar)](https://aur.archlinux.org/packages/tickerbar)
[![License: MIT](https://img.shields.io/github/license/mryll/tickerbar)](LICENSE)

A price ticker widget for [Waybar](https://github.com/Alexays/Waybar): crypto, the Argentine dollar (oficial/blue/MEP), and forex — in one module, with **no API key** for the core providers.

![screenshot](screenshot.png)

## Why tickerbar?

- **One widget, several markets.** Crypto, Argentine-peso rates, and forex pairs side by side, each from the best free source.
- **No API key for the core.** CoinGecko, DolarAPI, and Frankfurter all work without signing up for anything.
- **Never breaks your bar.** Any failure — a dead API, a rate limit, a malformed response, even a corrupt config — still prints valid Waybar JSON and exits 0. A down provider never blanks the others.
- **A tooltip worth opening.** A column-aligned table grouped by asset class, color-coded up/down, that picks up your [Omarchy](https://omarchy.org) theme colors automatically.

## Features

- Crypto prices with 24h change (CoinGecko) — pick the quote currency per asset (`usd`, `ars`, …)
- Argentine dollar: oficial, blue, MEP/bolsa, CCL, etc. (DolarAPI), buy/sell/mid side
- Forex pairs (Frankfurter v2)
- Stocks/indices with no key via CNBC (`AAPL`, `.SPX`); optional Finnhub with a free key
- Argentine market (BYMA) with no key via data912 — acciones, bonos, CEDEARs, ONs (in ARS)
- Multi-column tooltip (`tooltip_rows_per_column`) so large watchlists wrap instead of growing tall, with a column cap (`tooltip_max_columns`) that stacks extra columns into bands below — fits narrow/vertical monitors
- Market-hours aware: closed markets aren't polled (built-in calendars) and show `⏸ closed`
- Compact bar with a configurable format, or opt-in rotating "ticker-tape" mode
- Per-provider caching with TTLs and rate-limit (HTTP 429) backoff
- CSS classes for bar styling: lifecycle (`ok`/`partial`/`stale`/`error`) + direction (`up`/`down`/`flat`/`mixed`)
- Nerd Font, emoji, or ASCII icon sets
- Written in Rust — single binary, no runtime dependencies

## Requirements

- [Waybar](https://github.com/Alexays/Waybar)
- A [Nerd Font](https://www.nerdfonts.com/) for icons (or set `icons = "ascii"`)

## Installation

### Arch Linux (AUR)

```bash
yay -S tickerbar
```

### From source

```bash
git clone https://github.com/mryll/tickerbar.git
cd tickerbar
make install PREFIX=~/.local
```

Or system-wide: `sudo make install`.

## Configuration

tickerbar reads `~/.config/tickerbar/config.toml`. Copy the example to get started:

```bash
mkdir -p ~/.config/tickerbar
cp config.example.toml ~/.config/tickerbar/config.toml
```

```toml
[display]
mode = "fixed"          # "fixed" | "rotate"
rotate_interval = 5     # seconds per asset (rotate mode only)
max_on_bar = 3          # assets shown on the bar (fixed mode)
icons = "nerd"          # "nerd" | "emoji" | "ascii"
# Bar placeholders: {label} {price} {change_pct} {arrow} {glyph}
bar_format = "{glyph} {label} {price} {arrow}{change_pct}"

[[asset]]
label = "BTC"
provider = "coingecko"
id = "bitcoin"          # CoinGecko coin id
quote = "usd"           # any CoinGecko vs_currency (usd, ars, eur, …)

[[asset]]
label = "Blue"
provider = "dolarapi"
casa = "blue"           # oficial | blue | bolsa | contadoconliqui | tarjeta | mayorista | cripto
side = "sell"           # buy | sell | mid

[[asset]]
label = "EUR/USD"
provider = "frankfurter"
base = "eur"
quote = "usd"
```

### Providers

| Provider | Asset class | API key | Notes |
|---|---|---|---|
| `coingecko` | Crypto | No | `id` + `quote` (vs_currency). 24h change included. |
| `dolarapi` | Argentine peso | No | `casa` + `side`. Uses the provider's own update timestamp. |
| `frankfurter` | Forex | No | `base` + `quote`. Reference rates (daily). |
| `cnbc` | Stocks/indices | No | `symbol` — plain ticker (`AAPL`) or index with a leading dot (`.SPX`, `.IXIC`, `.DJI`). |
| `data912` | Argentine market (BYMA) | No | `panel` (`acciones`/`bonos`/`cedears`/`corp`) + `symbol` (e.g. `ALUA`, `GD35`, `MELI`). ARS, ~2h delay. |
| `finnhub` | Stocks/indices | Yes (free) | `symbol`. Token via `FINNHUB_TOKEN` env var. |
| `stooq` | Stocks/indices | No | `symbol` (e.g. `aapl.us`). Best-effort; often anti-bot-walled → `n/d`. |

> [!NOTE]
> **Stocks/indices work with no key via `cnbc`** (CNBC's public quote endpoint, batched). It's an unofficial endpoint, so treat it as delayed/best-effort — tickerbar is **not** a live trading feed. If you want a documented/keyed source instead, use `finnhub` with a free key:
>
> ```bash
> export FINNHUB_TOKEN=your_free_token   # from https://finnhub.io; sent as a header, never in a URL
> ```

> [!NOTE]
> `BTC/ARS` via CoinGecko uses CoinGecko's market ARS (≈ official), **not** the blue dollar. Pricing crypto at the blue rate (cross-conversion) is intentionally not done in this version.

### Market hours

By default tickerbar does **not** poll a market while it's closed — it serves the last close
from cache and marks the panel `⏸ closed` in the tooltip. Built-in calendars (timezone- and
DST-aware via `chrono-tz`): crypto 24/7; BYMA (data912) Mon–Fri 10:30–17:00 ART; US stocks
(cnbc/finnhub/stooq) Mon–Fri 09:30–16:00 ET; ECB forex (frankfurter) weekdays.

```toml
[market_hours]
enabled = true                 # master switch (default)

[market_hours.providers.cnbc]
enabled = false                # disable gating for one provider (e.g. a non-US stock via cnbc)
```

> [!NOTE]
> Stock providers (`cnbc`/`finnhub`/`stooq`) assume **US** hours; disable their gating if you
> track a non-US instrument through them. Exchange **holidays are not** accounted for — only
> weekly session hours.

## Waybar integration

Add a custom module to `~/.config/waybar/config`:

```jsonc
"custom/tickerbar": {
  "exec": "tickerbar",
  "return-type": "json",
  "interval": 60,
  "tooltip": true,
  "signal": 8
}
```

Then place `"custom/tickerbar"` in one of your `modules-*` arrays. Force a refresh anytime with:

```bash
pkill -RTMIN+8 waybar
```

Style it in `~/.config/waybar/style.css` using the emitted classes:

```css
#custom-tickerbar.up    { color: #98c379; }
#custom-tickerbar.down  { color: #e06c75; }
#custom-tickerbar.stale { opacity: 0.6; }
#custom-tickerbar.error { color: #e06c75; }
```

## Development

```bash
cargo test                          # unit + never-crash tests (offline)
cargo test --features integration   # real-API smoke tests (network; set FINNHUB_TOKEN for the finnhub one)
cargo clippy --all-targets -- -D warnings
cargo fmt
```
