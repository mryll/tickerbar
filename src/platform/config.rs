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
    /// Optional bar-level summary prepended to the bar text. Empty = off.
    /// Placeholders: {avg_change} {avg_arrow}. See `render::render_summary`.
    #[serde(default)]
    pub summary_format: String,
    /// Curated, ordered subset of asset labels shown on the bar. Empty = all assets (config order).
    /// Does not affect the tooltip.
    #[serde(default)]
    pub bar: Vec<String>,
    /// Layout template combining the assets block and the summary block. Placeholders: {bar} {summary}.
    #[serde(default = "default_bar_layout")]
    pub bar_layout: String,
    /// Wrap the tooltip into multiple columns every N lines. 0 = single column.
    #[serde(default)]
    pub tooltip_rows_per_column: usize,
    /// Cap columns shown side by side; extra columns wrap into a new stacked band below.
    /// 0 = unlimited (single band). Useful for narrow / vertical monitors.
    #[serde(default)]
    pub tooltip_max_columns: usize,
    /// Append a dim intraday low–high range to each tooltip row, where the provider supplies it
    /// (currently CNBC-backed assets). Off by default to keep the tooltip compact.
    #[serde(default)]
    pub tooltip_range: bool,
    /// DEPRECATED, still accepted so an existing config keeps loading. It drew
    /// a bordered card around the tooltip and now does nothing.
    #[serde(default)]
    #[allow(dead_code)]
    pub frame: bool,
    /// The tooltip is pinned to this family. A Pango family LIST, not one name:
    /// Pango tries them in order and falls through when one is not installed —
    /// the Arch package ttf-jetbrains-mono-nerd does NOT ship the "…Mono"
    /// family, so pinning only that name fell back to the system's proportional
    /// font without saying so.
    ///
    /// It must be monospace: the tooltip is a table, and its rules are
    /// box-drawing characters. In a proportional font the columns stop lining
    /// up and the rules render far wider than the text they underline, so the
    /// tooltip sizes itself to the rules and grows a dead margin on its right.
    /// Waybar draws the tooltip in a GTK window that ignores font-family from
    /// CSS, so the markup is the only place to say it.
    #[serde(default = "default_tooltip_font", alias = "frame_font")]
    pub tooltip_font: String,
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
    // The class mark leads, matching the Omarchy plugin's strip and the tooltip
    // group headers. It was absent here while the README documented it as the
    // default, so an unconfigured Waybar showed no glyph at all.
    "{glyph} {label} {price} {arrow}{change_pct}".into()
}
fn default_bar_layout() -> String {
    "{summary}   {bar}".into()
}
fn default_tooltip_font() -> String {
    "JetBrainsMono Nerd Font, JetBrainsMono Nerd Font Mono, monospace".into()
}

impl Display {
    /// Merge frontend-supplied layout overrides (the `--rows-per-column` / `--max-columns`
    /// CLI flags) into the config, so every downstream consumer — waybar tooltip and
    /// structured JSON alike — packs columns through the exact same values:
    /// - `rows_per_column`: an explicit config value (> 0) always wins; the override only
    ///   fills in when the config leaves it 0 (frontends pass their measured line budget).
    /// - `max_columns`: the smaller of the two positive values wins (a frontend can only
    ///   shrink what fits side by side, never widen past the config).
    pub fn apply_layout_overrides(
        &mut self,
        rows_per_column: Option<usize>,
        max_columns: Option<usize>,
    ) {
        if let Some(n) = rows_per_column {
            if self.tooltip_rows_per_column == 0 && n > 0 {
                self.tooltip_rows_per_column = n;
            }
        }
        if let Some(m) = max_columns {
            if m > 0 {
                self.tooltip_max_columns = if self.tooltip_max_columns > 0 {
                    self.tooltip_max_columns.min(m)
                } else {
                    m
                };
            }
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Display {
            mode: DisplayMode::default(),
            rotate_interval: default_rotate_interval(),
            max_on_bar: default_max_on_bar(),
            icons: default_icons(),
            bar_format: default_bar_format(),
            summary_format: String::new(),
            bar: Vec::new(),
            bar_layout: default_bar_layout(),
            tooltip_rows_per_column: 0,
            tooltip_max_columns: 0,
            tooltip_range: false,
            frame: false,
            tooltip_font: default_tooltip_font(),
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
        // Retired providers get a named error instead of serde's "unknown variant", so a
        // config written against an older release says what to do rather than what broke.
        // Stooq's quote endpoint (/q/l/) answers 404 since 2026 and the rest of the site is
        // behind a JavaScript proof-of-work gate, so there is no CLI-reachable feed left.
        if s.contains("\"stooq\"") {
            return Err(
                "provider 'stooq' was removed: its quote endpoint is gone (HTTP 404). \
                 Use provider = \"cnbc\" for stocks (no API key, e.g. symbol = \"AAPL\")."
                    .into(),
            );
        }
        let cfg: Config = toml::from_str(s).map_err(|e| format!("config parse error: {e}"))?;
        // Catch typoed provider keys in [market_hours.providers] instead of ignoring them.
        // Keep in sync with ProviderKind::as_str.
        const KNOWN_PROVIDERS: &[&str] = &[
            "coingecko",
            "dolarapi",
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
        // A missing file is the FIRST RUN, not a failure to report as one. It is
        // what every new user meets, and "No such file or directory" tells them
        // nothing they can act on — so this path says what to write and where the
        // shipped example is. Every other io error keeps its own words.
        let body = crate::platform::safe_read::read_bounded(
            &path,
            crate::platform::safe_read::CONFIG_LIMIT,
        )
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                let dir = path
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                // The example is named where it ACTUALLY is. A hardcoded
                // /usr/share is right for the package and wrong for
                // `make install PREFIX=~/.local` or a bare release binary,
                // and a copy command that names a missing file is worse
                // than no copy command.
                format!(
                    "no config yet: {}\n\nCopy the example and put your assets in it:\n  mkdir -p {}\n  cp {} {}",
                    path.display(),
                    dir,
                    example_path(),
                    path.display(),
                )
            } else {
                format!("cannot read config {}: {e}", path.display())
            }
        })?;
        Config::parse_str(&body)
    }
}

/// Where the shipped `config.example.toml` is, as a string the user can paste.
///
/// The package puts it under the install prefix, so the prefix is whatever
/// this binary was installed with — /usr for the package, ~/.local for
/// `make install PREFIX=~/.local`, and nowhere at all for a release binary
/// dropped on the PATH by hand. Each candidate is checked; the repository URL
/// is the answer when none of them is there.
fn example_path() -> String {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(prefix) = exe.parent().and_then(|d| d.parent()) {
            candidates.push(prefix.join("share/tickerbar/config.example.toml"));
        }
    }
    candidates.push(PathBuf::from("/usr/share/tickerbar/config.example.toml"));
    for c in candidates {
        if c.is_file() {
            return c.display().to_string();
        }
    }
    "https://github.com/mryll/tickerbar/raw/master/config.example.toml".into()
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
    fn summary_format_defaults_to_empty_and_parses_from_toml() {
        assert_eq!(Display::default().summary_format, "");
        let cfg = Config::parse_str(
            "[display]\nsummary_format = \"Σ {avg_arrow}{avg_change}\"\n\
             [[asset]]\nlabel = \"BTC\"\nprovider = \"coingecko\"\nid = \"bitcoin\"\nquote = \"usd\"\n",
        )
        .unwrap();
        assert_eq!(cfg.display.summary_format, "Σ {avg_arrow}{avg_change}");
    }

    #[test]
    fn bar_and_bar_layout_have_sensible_defaults_and_parse() {
        assert!(Display::default().bar.is_empty());
        assert_eq!(Display::default().bar_layout, "{summary}   {bar}");
        let cfg = Config::parse_str(
            "[display]\nbar = [\"BTC\", \"ETH\"]\nbar_layout = \"{bar} | {summary}\"\n\
             [[asset]]\nlabel = \"BTC\"\nprovider = \"coingecko\"\nid = \"bitcoin\"\nquote = \"usd\"\n",
        )
        .unwrap();
        assert_eq!(cfg.display.bar, vec!["BTC".to_string(), "ETH".to_string()]);
        assert_eq!(cfg.display.bar_layout, "{bar} | {summary}");
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
    fn layout_overrides_fill_auto_rows_but_never_beat_an_explicit_config() {
        let mut d = Display::default(); // rows_per_column = 0 (auto for frontends)
        d.apply_layout_overrides(Some(20), None);
        assert_eq!(d.tooltip_rows_per_column, 20);

        let mut pinned = Display {
            tooltip_rows_per_column: 14,
            ..Display::default()
        };
        pinned.apply_layout_overrides(Some(20), None);
        assert_eq!(pinned.tooltip_rows_per_column, 14, "explicit config wins");

        // A zero override is a no-op (0/absent = config value).
        let mut zero = Display::default();
        zero.apply_layout_overrides(Some(0), Some(0));
        assert_eq!(zero.tooltip_rows_per_column, 0);
        assert_eq!(zero.tooltip_max_columns, 0);
    }

    #[test]
    fn max_columns_override_takes_the_smaller_positive_value() {
        let mut d = Display {
            tooltip_max_columns: 3,
            ..Display::default()
        };
        d.apply_layout_overrides(None, Some(2));
        assert_eq!(d.tooltip_max_columns, 2, "frontend can shrink");
        d.apply_layout_overrides(None, Some(5));
        assert_eq!(d.tooltip_max_columns, 2, "frontend cannot widen");

        let mut unlimited = Display::default(); // max_columns = 0
        unlimited.apply_layout_overrides(None, Some(4));
        assert_eq!(unlimited.tooltip_max_columns, 4);
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
