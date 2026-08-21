use std::panic;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use tickerbar::platform::config::Config;
use tickerbar::platform::data;
use tickerbar::platform::http::Http;
use tickerbar::platform::presets;
use tickerbar::platform::render::{self, ColorMode};

use tickerbar::platform::theme::ThemeColors;
use tickerbar::platform::waybar::{self, WaybarOutput};
use tickerbar::providers;

/// Output format. `waybar` (default) is the classic Pango-marked module JSON; `json` is the
/// structured raw-data document consumed by frontends that render themselves (e.g. the
/// Omarchy shell plugin in `omarchy/`) — no markup, no colors, `schema_version`-ed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, ValueEnum)]
enum OutputFormat {
    #[default]
    Waybar,
    Json,
}

/// Which surface `--no-color` strips. `all` is what a bare `--no-color` means.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, ValueEnum)]
enum NoColorScope {
    #[default]
    All,
    Bar,
    Tooltip,
}

/// Resolve the two color surfaces. The explicit flag is the more specific instruction, so
/// it WINS over `NO_COLOR` — including `--no-color=bar`, which still colors the tooltip
/// with `NO_COLOR` set. Taking the env as a bool keeps this testable without mutating it.
fn color_mode(flag: Option<NoColorScope>, no_color_env: bool) -> ColorMode {
    match flag {
        Some(NoColorScope::All) => ColorMode::NONE,
        Some(NoColorScope::Bar) => ColorMode::PLAIN_BAR,
        Some(NoColorScope::Tooltip) => ColorMode::PLAIN_TOOLTIP,
        None if no_color_env => ColorMode::NONE,
        None => ColorMode::FULL,
    }
}

/// <https://no-color.org>: set to any NON-EMPTY value.
fn no_color_env() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

#[derive(Parser)]
#[command(
    name = "tickerbar",
    version,
    about = "Multi-market price ticker for Waybar (crypto, stocks, indices, commodities, forex, rates) — no API key"
)]
struct Cli {
    #[arg(
        long,
        help = "Path to config TOML (default: ~/.config/tickerbar/config.toml)"
    )]
    config: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = 2,
        help = "Per-provider HTTP timeout (seconds)"
    )]
    timeout: u64,

    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Waybar,
        help = "Output format: waybar (Pango module JSON) or json (structured raw data)"
    )]
    output: OutputFormat,

    #[arg(
        long,
        value_name = "N",
        help = "Column line budget used when the config leaves tooltip_rows_per_column at 0 \
                (frontends pass their measured fit; an explicit config value always wins; 0/absent = config)"
    )]
    rows_per_column: Option<usize>,

    #[arg(
        long,
        value_name = "N",
        help = "Clamp tooltip_max_columns (the smaller positive value wins; 0/absent = config)"
    )]
    max_columns: Option<usize>,

    #[arg(
        long,
        value_name = "NAME",
        help = "Print a ready-to-paste watchlist preset and exit (see --list-presets)"
    )]
    preset: Option<String>,

    #[arg(long, help = "List the available presets and exit")]
    list_presets: bool,

    #[arg(
        long,
        value_enum,
        value_name = "WHAT",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "all",
        help = "Drop Pango colors: all (default when the value is omitted), bar or tooltip. \
                Glyphs, layout and the CSS `class` are kept. Overrides NO_COLOR."
    )]
    no_color: Option<NoColorScope>,
}

/// Pre-scan of raw argv for a structured-output request, used ONLY when clap parsing fails:
/// the exit-0 fallback must speak the caller's requested format, and a failed parse yields no
/// `Cli`. Recognizes the exact pair `--output json` and `--output=json`; a trailing `--output`
/// with no value stays waybar (same as today).
fn requested_json_output(args: &[String]) -> bool {
    args.iter().enumerate().any(|(i, a)| {
        a == "--output=json"
            || (a == "--output" && args.get(i + 1).map(String::as_str) == Some("json"))
    })
}

/// Pre-scan of raw argv for an explicit `--no-color` scope, used ONLY when clap parsing
/// fails: a failed parse yields no `Cli`, but the scope is the more specific instruction
/// and must still outrank `NO_COLOR` on the exit-0 fallback. Mirrors the flag's
/// `require_equals` spelling; last occurrence wins, like clap. An unparseable value is no
/// scope at all, so the env var decides.
fn scanned_no_color(args: &[String]) -> Option<NoColorScope> {
    args.iter().rev().find_map(|a| match a.as_str() {
        "--no-color" | "--no-color=all" => Some(NoColorScope::All),
        "--no-color=bar" => Some(NoColorScope::Bar),
        "--no-color=tooltip" => Some(NoColorScope::Tooltip),
        _ => None,
    })
}

fn main() {
    // `try_parse` so a bad arg never panics the Waybar path. `--help`/`--version` (and the
    // manual `--preset`/`--list-presets` helpers below) are deliberate exceptions: they exit
    // via clap / print plain text (manual invocation only; Waybar passes no args). Any arg
    // ERROR maps to fallback JSON + exit 0, in the format the argv asked for.
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) if e.use_stderr() => {
            let args: Vec<String> = std::env::args().collect();
            let output = if requested_json_output(&args) {
                OutputFormat::Json
            } else {
                OutputFormat::Waybar
            };
            let mode = color_mode(scanned_no_color(&args), no_color_env());
            print_fallback("bad arguments", output, mode);
            return;
        }
        Err(e) => e.exit(),
    };

    // Manual-invocation helpers: emit a preset (or the list) and exit, like --help. Waybar
    // passes no args, so these never affect the JSON path.
    if cli.list_presets {
        for name in presets::NAMES {
            println!("{name}");
        }
        return;
    }
    if let Some(name) = &cli.preset {
        match presets::preset(name) {
            Some(body) => print!("{body}"),
            None => {
                eprintln!(
                    "unknown preset '{name}'. Available: {}",
                    presets::NAMES.join(", ")
                );
                std::process::exit(2);
            }
        }
        return;
    }

    let mode = color_mode(cli.no_color, no_color_env());
    let colors = ThemeColors::load();
    let json = panic::catch_unwind(panic::AssertUnwindSafe(|| run_to_json(&cli, &colors, mode)))
        .unwrap_or_else(|_| fallback_json("internal error", cli.output, &colors, mode));
    println!("{json}");
}

fn run_to_json(cli: &Cli, colors: &ThemeColors, mode: ColorMode) -> String {
    match Config::load(cli.config.as_ref()) {
        Ok(mut cfg) => {
            // Layout overrides land in the shared config BEFORE any rendering, so the
            // waybar tooltip and the structured JSON pack columns through the exact
            // same values and the exact same code path (render::pack_columns).
            cfg.display
                .apply_layout_overrides(cli.rows_per_column, cli.max_columns);
            let http = Http::new(cli.timeout);
            let now = chrono::Utc::now();
            let quotes = providers::fetch_all(&cfg.assets, &http, now, &cfg.market_hours);
            match cli.output {
                // The structured document carries raw data and `state`, never presentation:
                // `--no-color` deliberately does NOT reach it.
                OutputFormat::Waybar => to_json(render::build(&cfg, &quotes, now, colors, mode)),
                OutputFormat::Json => data_to_json(data::build(&cfg, &quotes, now, colors)),
            }
        }
        Err(msg) => fallback_json(&msg, cli.output, colors, mode),
    }
}

fn fallback_json(msg: &str, output: OutputFormat, colors: &ThemeColors, mode: ColorMode) -> String {
    match output {
        OutputFormat::Waybar => {
            // The error tooltip is a tooltip surface, so it follows the tooltip setting.
            let mono = ThemeColors::monochrome();
            let c = if mode.tooltip() { colors } else { &mono };
            to_json(waybar::error_output("tickerbar error", msg, c))
        }
        // The structured document is presentation-free: it always publishes the REAL
        // palette, never the monochrome one, so `--no-color` cannot alter it.
        OutputFormat::Json => data_to_json(data::error_output(msg, chrono::Utc::now(), colors)),
    }
}

fn to_json(o: WaybarOutput) -> String {
    serde_json::to_string(&o).unwrap_or_else(|_| {
        r#"{"text":"?","tooltip":"serialization error","class":["error"],"alt":"error"}"#
            .to_string()
    })
}

fn data_to_json(o: data::DataOutput) -> String {
    // The hand-written fallback must stay schema-complete, palette included.
    let p = data::Palette::from_theme(&ThemeColors::default());
    serde_json::to_string(&o).unwrap_or_else(|_| {
        format!(
            r#"{{"schema_version":{},"state":"error","error":{{"message":"serialization error"}},"fetched_at":"","bar":[],"avg_change_pct":null,"summary_configured":false,"layout":{{"rows_per_column":0,"max_columns":0,"bands":[]}},"groups":[],"palette":{{"up":"{}","down":"{}","flat":"{}","text":"{}","dim":"{}","accent":"{}","error":"{}"}}}}"#,
            data::SCHEMA_VERSION,
            p.up,
            p.down,
            p.flat,
            p.text,
            p.dim,
            p.accent,
            p.error
        )
    })
}

fn print_fallback(msg: &str, output: OutputFormat, mode: ColorMode) {
    // Argv failed to parse, so the theme is not loaded on this path: the built-in palette
    // keeps the fallback cheap and infallible. The scope comes from the argv pre-scan.
    let colors = ThemeColors::default();
    println!("{}", fallback_json(msg, output, &colors, mode));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_surfaces_are_colored_when_nothing_asks_otherwise() {
        assert_eq!(color_mode(None, false), ColorMode::FULL);
    }

    #[test]
    fn a_bare_no_color_flag_strips_both_surfaces() {
        assert_eq!(color_mode(Some(NoColorScope::All), false), ColorMode::NONE);
    }

    #[test]
    fn scoping_the_flag_strips_only_that_surface() {
        let bar = color_mode(Some(NoColorScope::Bar), false);
        assert!(!bar.bar(), "--no-color=bar leaves the bar plain");
        assert!(bar.tooltip(), "...and the tooltip colored");

        let tooltip = color_mode(Some(NoColorScope::Tooltip), false);
        assert!(tooltip.bar());
        assert!(!tooltip.tooltip());
    }

    #[test]
    fn the_no_color_environment_variable_strips_both_surfaces() {
        assert_eq!(color_mode(None, true), ColorMode::NONE);
    }

    #[test]
    fn an_explicit_scope_survives_an_argument_error() {
        // Clap never produced a `Cli`, but the scope is still the more specific
        // instruction, so the fallback must not fall back to plain NO_COLOR behaviour.
        let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let mode = color_mode(
            scanned_no_color(&argv(&["tickerbar", "--no-color=bar", "--bogus"])),
            true,
        );
        assert!(!mode.bar());
        assert!(mode.tooltip(), "NO_COLOR must not widen an explicit scope");

        assert_eq!(scanned_no_color(&argv(&["tickerbar"])), None);
        assert_eq!(
            scanned_no_color(&argv(&["tickerbar", "--no-color"])),
            Some(NoColorScope::All)
        );
        // Last occurrence wins, like clap.
        assert_eq!(
            scanned_no_color(&argv(&[
                "tickerbar",
                "--no-color=bar",
                "--no-color=tooltip"
            ])),
            Some(NoColorScope::Tooltip)
        );
        // An unparseable value is no scope at all, so the env var decides.
        assert_eq!(
            scanned_no_color(&argv(&["tickerbar", "--no-color=zzz"])),
            None
        );
    }

    #[test]
    fn an_explicit_scope_wins_over_the_environment_variable() {
        // NO_COLOR is set, but `--no-color=bar` is the more specific instruction, so the
        // tooltip keeps its colors.
        let mode = color_mode(Some(NoColorScope::Bar), true);
        assert!(!mode.bar());
        assert!(mode.tooltip(), "the explicit scope must beat NO_COLOR");
    }
}
