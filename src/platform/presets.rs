//! Curated, ready-to-paste watchlists emitted by `--preset <name>`.
//!
//! Each preset is a block of TOML `[[asset]]` entries (no `[display]`), so it can be appended
//! to an existing config:  `tickerbar --preset crypto-top >> ~/.config/tickerbar/config.toml`.
//! Symbols use the friendly providers, so the snippets stay readable.

/// Preset names, in listing order.
pub const NAMES: &[&str] = &[
    "starter",
    "crypto-top",
    "megacap",
    "indices-global",
    "fx-majors",
    "commodities",
    "rates",
];

/// The TOML body for a preset, or `None` if the name is unknown.
pub fn preset(name: &str) -> Option<&'static str> {
    Some(match name {
        "starter" => STARTER,
        "crypto-top" => CRYPTO_TOP,
        "megacap" => MEGACAP,
        "indices-global" => INDICES_GLOBAL,
        "fx-majors" => FX_MAJORS,
        "commodities" => COMMODITIES,
        "rates" => RATES,
        _ => return None,
    })
}

const STARTER: &str = r#"# tickerbar preset: starter — a balanced cross-market watchlist
[[asset]]
label = "BTC"
provider = "coingecko"
id = "bitcoin"
quote = "usd"

[[asset]]
label = "ETH"
provider = "coingecko"
id = "ethereum"
quote = "usd"

[[asset]]
label = "NVDA"
provider = "cnbc"
symbol = "NVDA"

[[asset]]
label = "S&P 500"
provider = "index"
symbol = "sp500"

[[asset]]
label = "Gold"
provider = "commodity"
symbol = "gold"

[[asset]]
label = "US 10Y"
provider = "rate"
symbol = "us10y"

[[asset]]
label = "EUR/USD"
provider = "frankfurter"
base = "eur"
quote = "usd"
"#;

const CRYPTO_TOP: &str = r#"# tickerbar preset: crypto-top — largest cryptocurrencies by market cap
[[asset]]
label = "BTC"
provider = "coingecko"
id = "bitcoin"
quote = "usd"

[[asset]]
label = "ETH"
provider = "coingecko"
id = "ethereum"
quote = "usd"

[[asset]]
label = "SOL"
provider = "coingecko"
id = "solana"
quote = "usd"

[[asset]]
label = "XRP"
provider = "coingecko"
id = "ripple"
quote = "usd"

[[asset]]
label = "BNB"
provider = "coingecko"
id = "binancecoin"
quote = "usd"

[[asset]]
label = "ADA"
provider = "coingecko"
id = "cardano"
quote = "usd"

[[asset]]
label = "LINK"
provider = "coingecko"
id = "chainlink"
quote = "usd"

[[asset]]
label = "AVAX"
provider = "coingecko"
id = "avalanche-2"
quote = "usd"
"#;

const MEGACAP: &str = r#"# tickerbar preset: megacap — the largest US tech companies (CNBC)
[[asset]]
label = "NVDA"
provider = "cnbc"
symbol = "NVDA"

[[asset]]
label = "AAPL"
provider = "cnbc"
symbol = "AAPL"

[[asset]]
label = "MSFT"
provider = "cnbc"
symbol = "MSFT"

[[asset]]
label = "GOOGL"
provider = "cnbc"
symbol = "GOOGL"

[[asset]]
label = "AMZN"
provider = "cnbc"
symbol = "AMZN"

[[asset]]
label = "META"
provider = "cnbc"
symbol = "META"

[[asset]]
label = "TSLA"
provider = "cnbc"
symbol = "TSLA"
"#;

const INDICES_GLOBAL: &str = r#"# tickerbar preset: indices-global — major world indices + volatility (CNBC)
[[asset]]
label = "S&P 500"
provider = "index"
symbol = "sp500"

[[asset]]
label = "Nasdaq"
provider = "index"
symbol = "nasdaq"

[[asset]]
label = "Dow"
provider = "index"
symbol = "dow"

[[asset]]
label = "VIX"
provider = "index"
symbol = "vix"

[[asset]]
label = "DAX"
provider = "index"
symbol = "dax"

[[asset]]
label = "FTSE 100"
provider = "index"
symbol = "ftse"

[[asset]]
label = "Nikkei"
provider = "index"
symbol = "nikkei"

[[asset]]
label = "Hang Seng"
provider = "index"
symbol = "hangseng"
"#;

const FX_MAJORS: &str = r#"# tickerbar preset: fx-majors — most-traded forex pairs (ECB / Frankfurter)
[[asset]]
label = "EUR/USD"
provider = "frankfurter"
base = "eur"
quote = "usd"

[[asset]]
label = "USD/JPY"
provider = "frankfurter"
base = "usd"
quote = "jpy"

[[asset]]
label = "GBP/USD"
provider = "frankfurter"
base = "gbp"
quote = "usd"

[[asset]]
label = "AUD/USD"
provider = "frankfurter"
base = "aud"
quote = "usd"

[[asset]]
label = "USD/CAD"
provider = "frankfurter"
base = "usd"
quote = "cad"

[[asset]]
label = "USD/CHF"
provider = "frankfurter"
base = "usd"
quote = "chf"
"#;

const COMMODITIES: &str = r#"# tickerbar preset: commodities — metals & energy (CNBC)
[[asset]]
label = "Gold"
provider = "commodity"
symbol = "gold"

[[asset]]
label = "Silver"
provider = "commodity"
symbol = "silver"

[[asset]]
label = "WTI Crude"
provider = "commodity"
symbol = "wti"

[[asset]]
label = "Brent"
provider = "commodity"
symbol = "brent"

[[asset]]
label = "Nat Gas"
provider = "commodity"
symbol = "natgas"

[[asset]]
label = "Copper"
provider = "commodity"
symbol = "copper"
"#;

const RATES: &str = r#"# tickerbar preset: rates — US Treasury yields (CNBC), shown as a percent
[[asset]]
label = "US 2Y"
provider = "rate"
symbol = "us2y"

[[asset]]
label = "US 5Y"
provider = "rate"
symbol = "us5y"

[[asset]]
label = "US 10Y"
provider = "rate"
symbol = "us10y"

[[asset]]
label = "US 30Y"
provider = "rate"
symbol = "us30y"
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::config::Config;

    #[test]
    fn an_unknown_preset_is_none() {
        assert!(preset("nope").is_none());
    }

    #[test]
    fn every_listed_preset_parses_as_a_valid_config() {
        for name in NAMES {
            let body = preset(name).unwrap_or_else(|| panic!("missing preset body: {name}"));
            let cfg = Config::parse_str(body)
                .unwrap_or_else(|e| panic!("preset {name} is not valid TOML config: {e}"));
            assert!(!cfg.assets.is_empty(), "preset {name} produced no assets");
        }
    }
}
