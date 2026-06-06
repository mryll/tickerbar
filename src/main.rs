use std::panic;
use std::path::PathBuf;

use clap::Parser;

use tickerbar::platform::config::Config;
use tickerbar::platform::http::Http;
use tickerbar::platform::presets;
use tickerbar::platform::render;
use tickerbar::platform::theme::ThemeColors;
use tickerbar::platform::waybar::{self, WaybarOutput};
use tickerbar::providers;

#[derive(Parser)]
#[command(
    name = "tickerbar",
    version,
    about = "Price ticker widget for Waybar (crypto, fiat/ARS, stocks)"
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
        value_name = "NAME",
        help = "Print a ready-to-paste watchlist preset and exit (see --list-presets)"
    )]
    preset: Option<String>,

    #[arg(long, help = "List the available presets and exit")]
    list_presets: bool,
}

fn main() {
    // `try_parse` so a bad arg never panics the Waybar path. `--help`/`--version` are a
    // deliberate exception: they exit via clap (manual invocation only; Waybar passes no
    // args). Any arg ERROR maps to fallback JSON + exit 0.
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) if e.use_stderr() => {
            print_fallback("bad arguments");
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

    let colors = ThemeColors::load();
    let json = panic::catch_unwind(panic::AssertUnwindSafe(|| run_to_json(&cli, &colors)))
        .unwrap_or_else(|_| fallback_json("internal error", &colors));
    println!("{json}");
}

fn run_to_json(cli: &Cli, colors: &ThemeColors) -> String {
    match Config::load(cli.config.as_ref()) {
        Ok(cfg) => {
            let http = Http::new(cli.timeout);
            let now = chrono::Utc::now();
            let quotes = providers::fetch_all(&cfg.assets, &http, now, &cfg.market_hours);
            to_json(render::build(&cfg, &quotes, now, colors))
        }
        Err(msg) => fallback_json(&msg, colors),
    }
}

fn fallback_json(msg: &str, colors: &ThemeColors) -> String {
    to_json(waybar::error_output("tickerbar error", msg, colors))
}

fn to_json(o: WaybarOutput) -> String {
    serde_json::to_string(&o).unwrap_or_else(|_| {
        r#"{"text":"?","tooltip":"serialization error","class":["error"],"alt":"error"}"#
            .to_string()
    })
}

fn print_fallback(msg: &str) {
    let colors = ThemeColors::default();
    println!("{}", fallback_json(msg, &colors));
}
