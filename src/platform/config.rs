use std::path::PathBuf;

use serde::Deserialize;

use crate::platform::model::Asset;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum DisplayMode {
    #[default]
    Fixed,
    Rotate,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Display {
    #[serde(default)]
    pub mode: DisplayMode,
    #[serde(default = "default_rotate_interval")]
    pub rotate_interval: u64,
    #[serde(default = "default_max_on_bar")]
    pub max_on_bar: usize,
    #[serde(default = "default_icons")]
    pub icons: String,
    #[serde(default = "default_bar_format")]
    pub bar_format: String,
    /// Wrap the tooltip into multiple columns every N lines. 0 = single column.
    #[serde(default)]
    pub tooltip_rows_per_column: usize,
    /// Cap columns shown side by side; extra columns wrap into a new stacked band below.
    /// 0 = unlimited (single band). Useful for narrow / vertical monitors.
    #[serde(default)]
    pub tooltip_max_columns: usize,
}

fn default_rotate_interval() -> u64 {
    5
}
fn default_max_on_bar() -> usize {
    3
}
fn default_icons() -> String {
    "nerd".into()
}
fn default_bar_format() -> String {
    "{label} {price} {arrow}{change_pct}".into()
}

impl Default for Display {
    fn default() -> Self {
        Display {
            mode: DisplayMode::default(),
            rotate_interval: default_rotate_interval(),
            max_on_bar: default_max_on_bar(),
            icons: default_icons(),
            bar_format: default_bar_format(),
            tooltip_rows_per_column: 0,
            tooltip_max_columns: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderToggle {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarketHours {
    /// Master switch for market-hours gating.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Per-provider overrides, keyed by provider name (e.g. "cnbc").
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderToggle>,
}

impl Default for MarketHours {
    fn default() -> Self {
        MarketHours {
            enabled: true,
            providers: std::collections::HashMap::new(),
        }
    }
}

impl MarketHours {
    /// Whether gating applies to a given provider (master switch AND not disabled for it).
    pub fn applies_to(&self, provider: &str) -> bool {
        self.enabled
            && self
                .providers
                .get(provider)
                .map(|t| t.enabled)
                .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub display: Display,
    #[serde(default)]
    pub market_hours: MarketHours,
    #[serde(default, rename = "asset")]
    pub assets: Vec<Asset>,
}

impl Config {
    pub fn parse_str(s: &str) -> Result<Config, String> {
        let cfg: Config = toml::from_str(s).map_err(|e| format!("config parse error: {e}"))?;
        // Catch typoed provider keys in [market_hours.providers] instead of ignoring them.
        // Keep in sync with ProviderKind::as_str.
        const KNOWN_PROVIDERS: &[&str] = &[
            "coingecko",
            "dolarapi",
            "stooq",
            "frankfurter",
            "finnhub",
            "cnbc",
            "data912",
        ];
        for k in cfg.market_hours.providers.keys() {
            if !KNOWN_PROVIDERS.contains(&k.as_str()) {
                return Err(format!(
                    "unknown provider in [market_hours.providers]: '{k}'"
                ));
            }
        }
        Ok(cfg)
    }

    pub fn load(path: Option<&PathBuf>) -> Result<Config, String> {
        let path = match path {
            Some(p) => p.clone(),
            None => default_config_path(),
        };
        let body = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
        Config::parse_str(&body)
    }
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("tickerbar/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[display]
mode = "fixed"
max_on_bar = 2

[[asset]]
label = "BTC"
provider = "coingecko"
id = "bitcoin"
quote = "usd"

[[asset]]
label = "Blue"
provider = "dolarapi"
casa = "blue"
side = "sell"
"#;

    #[test]
    fn a_valid_config_parses_assets_and_display() {
        let cfg = Config::parse_str(SAMPLE).unwrap();
        assert_eq!(cfg.assets.len(), 2);
        assert_eq!(cfg.display.max_on_bar, 2);
        assert!(matches!(cfg.display.mode, DisplayMode::Fixed));
    }

    #[test]
    fn an_unknown_provider_is_rejected() {
        let bad = "[[asset]]\nlabel=\"x\"\nprovider=\"nasdaq\"\nsymbol=\"x\"\n";
        assert!(Config::parse_str(bad).is_err());
    }

    #[test]
    fn a_typoed_market_hours_provider_key_is_rejected() {
        let bad = "[market_hours.providers.cncb]\nenabled = false\n";
        assert!(Config::parse_str(bad).is_err());
    }

    #[test]
    fn a_known_market_hours_provider_key_is_accepted() {
        let ok = "[market_hours.providers.cnbc]\nenabled = false\n";
        let cfg = Config::parse_str(ok).unwrap();
        assert!(!cfg.market_hours.applies_to("cnbc"));
        assert!(cfg.market_hours.applies_to("coingecko"));
    }

    #[test]
    fn display_defaults_apply_when_section_is_absent() {
        let only_asset =
            "[[asset]]\nlabel=\"BTC\"\nprovider=\"coingecko\"\nid=\"bitcoin\"\nquote=\"usd\"\n";
        let cfg = Config::parse_str(only_asset).unwrap();
        assert!(matches!(cfg.display.mode, DisplayMode::Fixed));
        assert_eq!(cfg.display.max_on_bar, 3);
    }
}
