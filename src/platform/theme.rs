use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use crate::platform::safe_read;

/// Theme colors read from Omarchy, falling back to One Dark. Reused from meteobar, with an
/// added `red` for the "down" direction (meteobar only needed `error`).
pub struct ThemeColors {
    pub border: String,
    pub text: String,
    pub dim: String,
    pub accent: String,
    pub green: String,
    pub red: String,
    pub yellow: String,
    pub orange: String,
    pub error: String,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            border: "#61afef".into(),
            text: "#abb2bf".into(),
            dim: "#5c6370".into(),
            accent: "#61afef".into(),
            green: "#98c379".into(),
            red: "#e06c75".into(),
            yellow: "#e5c07b".into(),
            orange: "#d19a66".into(),
            error: "#e06c75".into(),
        }
    }
}

impl ThemeColors {
    /// The monochrome palette used by `--no-color`: every slot empty, which makes
    /// `waybar::fg`/`bold_fg` emit their text with no `foreground=` attribute. Routing
    /// monochrome through the palette (instead of through every call site) means a
    /// surface cannot accidentally keep a tint — there is no other source of color.
    pub fn monochrome() -> Self {
        Self {
            border: String::new(),
            text: String::new(),
            dim: String::new(),
            accent: String::new(),
            green: String::new(),
            red: String::new(),
            yellow: String::new(),
            orange: String::new(),
            error: String::new(),
        }
    }

    pub fn load() -> Self {
        Self::load_from(
            xdg_home(env::var_os("XDG_STATE_HOME"), ".local/state").as_deref(),
            dirs::config_dir().as_deref(),
            xdg_home(env::var_os("XDG_CACHE_HOME"), ".cache").as_deref(),
        )
    }

    /// Resolution chain: Omarchy theme (state dir, then legacy config dir) → pywal cache →
    /// built-in defaults. Never fails: a missing theme, an unreadable file or an
    /// unparseable palette all degrade to the next source (never-crash invariant).
    fn load_from(
        state_home: Option<&Path>,
        config_home: Option<&Path>,
        cache_home: Option<&Path>,
    ) -> Self {
        if let Some(path) = colors_file(state_home, config_home) {
            if let Ok(content) = safe_read::read_bounded(&path, safe_read::CONFIG_LIMIT) {
                return Self::from_map(&parse_toml_flat(&content));
            }
        }
        // Only when no Omarchy theme was found. One path covers three ecosystems:
        // archived pywal, the maintained pywal16 fork, and wallust's pywal-compat target
        // all write `<cache>/wal/colors.json`.
        if let Some(root) = cache_home {
            if let Ok(content) =
                safe_read::read_bounded(&root.join("wal/colors.json"), safe_read::CONFIG_LIMIT)
            {
                if let Some(colors) = Self::from_pywal(&content) {
                    return colors;
                }
            }
        }
        Self::default()
    }

    /// A pywal cache, mapped onto the same named keys the Omarchy path uses so the
    /// per-key independence and the hex guard are shared. `None` only when the file is
    /// not valid JSON; a valid but sparse document just keeps the missing defaults.
    fn from_pywal(json: &str) -> Option<Self> {
        let doc: serde_json::Value = serde_json::from_str(json).ok()?;

        let mut map = HashMap::new();
        let mut put = |key: &str, value: Option<&str>| {
            if let Some(value) = value {
                map.insert(key.to_string(), value.to_string());
            }
        };

        let (yellow, red) = (wal(&doc, "colors", "color3"), wal(&doc, "colors", "color1"));
        // pywal has no orange slot. The midpoint of yellow and red keeps a distinct
        // fourth stop; aliasing it to red would flatten the gauge ramps.
        let orange = match (yellow, red) {
            (Some(y), Some(r)) => Some(blend_hex(y, r, 0.5)),
            _ => None,
        };

        put("foreground", wal(&doc, "special", "foreground"));
        put("background", wal(&doc, "special", "background"));
        put("red", red);
        put("green", wal(&doc, "colors", "color2"));
        put("yellow", yellow);
        put(
            "accent",
            wal(&doc, "colors", "color4").or_else(|| wal(&doc, "special", "cursor")),
        );
        put("orange", orange.as_deref());

        Some(Self::from_map(&map))
    }

    /// Named keys win; `color1/2/3` keep legacy themes working exactly as before (there
    /// `color1` stood in for both red and orange). Any key that is absent keeps its
    /// default instead of dropping the whole theme.
    fn from_map(map: &HashMap<String, String>) -> Self {
        let d = Self::default();
        // Colors land verbatim in Pango markup, so only well-formed hex is accepted; a
        // junk value falls back to its default instead of breaking the tooltip.
        let get = |k: &str| map.get(k).map(String::as_str).filter(|v| is_hex_color(v));

        let accent = get("accent");
        let foreground = get("foreground");
        let background = get("background");
        let red = get("red").or_else(|| get("color1"));
        let green = get("green").or_else(|| get("color2"));
        let yellow = get("yellow").or_else(|| get("color3"));
        let orange = get("orange").or(red);

        let pick = |v: Option<&str>, fallback: &str| v.unwrap_or(fallback).to_string();
        let red = pick(red, &d.red);
        let dim = match (foreground, background) {
            (Some(fg), Some(bg)) => blend_hex(fg, bg, 0.5),
            _ => d.dim.clone(),
        };

        Self {
            border: pick(accent, &d.border),
            accent: pick(accent, &d.accent),
            text: pick(foreground, &d.text),
            dim,
            green: pick(green, &d.green),
            error: red.clone(),
            red,
            yellow: pick(yellow, &d.yellow),
            orange: pick(orange, &d.orange),
        }
    }
}

/// An XDG base dir: the env var when set and non-empty, else `~/<fallback>`. Taking the
/// raw var as an argument keeps the rule testable without mutating the environment.
/// `XDG_STATE_HOME` is resolved the same way the shell's own `Commons/Color.qml` does it.
fn xdg_home(var: Option<std::ffi::OsString>, fallback: &str) -> Option<PathBuf> {
    var.filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(fallback)))
}

/// One `#rrggbb` value out of a pywal document, guarded so junk never reaches Pango.
fn wal<'a>(doc: &'a serde_json::Value, section: &str, key: &str) -> Option<&'a str> {
    doc.get(section)?
        .get(key)?
        .as_str()
        .filter(|v| is_hex_color(v))
}

/// The active theme's palette: the state dir first (current Omarchy), then the config dir
/// (pre-state-dir installs). `None` when neither exists — a plain Waybar user.
fn colors_file(state_home: Option<&Path>, config_home: Option<&Path>) -> Option<PathBuf> {
    [state_home, config_home]
        .into_iter()
        .flatten()
        .map(|root| root.join("omarchy/current/theme/colors.toml"))
        .find(|p| p.is_file())
}

fn parse_toml_flat(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().trim_matches('"').to_string();
            map.insert(key, value);
        }
    }
    map
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` — the forms Pango understands.
fn is_hex_color(v: &str) -> bool {
    match v.strip_prefix('#') {
        Some(d) => matches!(d.len(), 3 | 4 | 6 | 8) && d.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}

/// RGB channels from any form `is_hex_color` accepts — the short `#rgb`/`#rgba` included,
/// so a theme written in shorthand still derives a real `dim` instead of silently blending
/// the One Dark constants. Alpha is parsed but dropped: the blend result is opaque.
fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let d = hex.strip_prefix('#')?.as_bytes();
    if !d.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let nibble = |b: u8| (b as char).to_digit(16).map(|v| v as u8);
    match d.len() {
        // Shorthand: each digit is doubled (`#abc` == `#aabbcc`).
        3 | 4 => {
            let dup = |i: usize| nibble(d[i]).map(|v| v * 17);
            Some((dup(0)?, dup(1)?, dup(2)?))
        }
        6 | 8 => {
            let byte = |i: usize| Some(nibble(d[i])? * 16 + nibble(d[i + 1])?);
            Some((byte(0)?, byte(2)?, byte(4)?))
        }
        _ => None,
    }
}

fn blend_hex(c1: &str, c2: &str, ratio: f32) -> String {
    let (r1, g1, b1) = parse_hex(c1).unwrap_or((171, 178, 191));
    let (r2, g2, b2) = parse_hex(c2).unwrap_or((40, 44, 52));
    let blend =
        |a: u8, b: u8| -> u8 { (a as f32 * (1.0 - ratio) + b as f32 * ratio).round() as u8 };
    format!(
        "#{:02x}{:02x}{:02x}",
        blend(r1, r2),
        blend(g1, g2),
        blend(b1, b2)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Tokyo Night, as current Omarchy ships it: semantic keys only, no `colorN`.
    const NAMED: &str = r##"
mode = "dark"

accent = "#7aa2f7"
background = "#1a1b26"
foreground = "#a9b1d6"

red = "#f7768e"
yellow = "#e0af68"
orange = "#eb927b"
green = "#9ece6a"
bright_red = "#ff7a93"
"##;

    /// A pre-semantic theme: terminal palette slots only.
    const LEGACY: &str = r##"
accent = "#61afef"
background = "#282c34"
foreground = "#abb2bf"
color1 = "#e06c75"
color2 = "#98c379"
color3 = "#e5c07b"
"##;

    fn scratch(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tickerbar-theme-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// A pywal cache as pywal / pywal16 / wallust write it.
    const PYWAL: &str = r##"{
  "wallpaper": "/home/u/wall.png",
  "alpha": "100",
  "special": {
    "background": "#11121a",
    "foreground": "#c0caf5",
    "cursor": "#bb9af7"
  },
  "colors": {
    "color0": "#11121a",
    "color1": "#f7768e",
    "color2": "#9ece6a",
    "color3": "#e0af68",
    "color4": "#7aa2f7",
    "color5": "#ad8ee6",
    "color6": "#449dab",
    "color7": "#a9b1d6"
  }
}"##;

    fn write_theme(root: &Path, body: &str) {
        let dir = root.join("omarchy/current/theme");
        fs::create_dir_all(&dir).expect("theme dir");
        fs::write(dir.join("colors.toml"), body).expect("colors.toml");
    }

    fn write_pywal(cache: &Path, body: &str) {
        let dir = cache.join("wal");
        fs::create_dir_all(&dir).expect("wal dir");
        fs::write(dir.join("colors.json"), body).expect("colors.json");
    }

    fn map_of(body: &str) -> HashMap<String, String> {
        parse_toml_flat(body)
    }

    #[test]
    fn palette_comes_from_the_state_dir() {
        let root = scratch("state");
        let state = root.join("state");
        write_theme(&state, NAMED);

        let c = ThemeColors::load_from(Some(&state), Some(&root.join("config")), None);

        assert_eq!(c.accent, "#7aa2f7");
        assert_eq!(c.red, "#f7768e");
        assert_eq!(c.green, "#9ece6a");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn state_dir_wins_over_the_legacy_config_dir() {
        let root = scratch("both");
        let (state, config) = (root.join("state"), root.join("config"));
        write_theme(&state, NAMED);
        write_theme(&config, LEGACY);

        let c = ThemeColors::load_from(Some(&state), Some(&config), None);

        assert_eq!(c.accent, "#7aa2f7", "state dir must take precedence");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_config_dir_still_works_when_the_state_dir_has_no_theme() {
        let root = scratch("legacy-path");
        let (state, config) = (root.join("state"), root.join("config"));
        fs::create_dir_all(&state).expect("empty state dir");
        write_theme(&config, LEGACY);

        let c = ThemeColors::load_from(Some(&state), Some(&config), None);

        assert_eq!(c.accent, "#61afef");
        assert_eq!(c.red, "#e06c75");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn defaults_are_used_when_no_theme_is_installed() {
        let root = scratch("none");
        let d = ThemeColors::default();

        let c = ThemeColors::load_from(
            Some(&root.join("state")),
            Some(&root.join("config")),
            Some(&root.join("cache")),
        );

        assert_eq!(c.accent, d.accent);
        assert_eq!(c.red, d.red);
        assert_eq!(c.dim, d.dim);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn defaults_are_used_when_no_home_can_be_resolved() {
        let c = ThemeColors::load_from(None, None, None);
        assert_eq!(c.accent, ThemeColors::default().accent);
    }

    #[test]
    fn pywal_is_used_when_no_omarchy_theme_is_installed() {
        let root = scratch("pywal-only");
        let cache = root.join("cache");
        write_pywal(&cache, PYWAL);

        let c = ThemeColors::load_from(
            Some(&root.join("state")),
            Some(&root.join("config")),
            Some(&cache),
        );

        assert_eq!(c.accent, "#7aa2f7", "color4");
        assert_eq!(c.border, "#7aa2f7");
        assert_eq!(c.text, "#c0caf5", "special.foreground");
        assert_eq!(c.red, "#f7768e", "color1");
        assert_eq!(c.error, "#f7768e");
        assert_eq!(c.green, "#9ece6a", "color2");
        assert_eq!(c.yellow, "#e0af68", "color3");
        assert_eq!(
            c.dim,
            blend_hex("#c0caf5", "#11121a", 0.5),
            "dim still blends fg into bg"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_omarchy_theme_wins_over_a_pywal_cache() {
        let root = scratch("omarchy-over-pywal");
        let (state, cache) = (root.join("state"), root.join("cache"));
        write_theme(&state, LEGACY);
        write_pywal(&cache, PYWAL);

        let c = ThemeColors::load_from(Some(&state), Some(&root.join("config")), Some(&cache));

        assert_eq!(c.accent, "#61afef", "pywal is only a fallback");
        assert_eq!(c.red, "#e06c75");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pywal_cache_that_is_not_json_falls_back_to_the_defaults() {
        let root = scratch("pywal-garbage");
        let cache = root.join("cache");
        write_pywal(&cache, "not json at all {{{");
        let d = ThemeColors::default();

        let c = ThemeColors::load_from(None, None, Some(&cache));

        assert_eq!(c.accent, d.accent);
        assert_eq!(c.red, d.red);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_pywal_cache_falls_back_to_the_defaults() {
        let root = scratch("pywal-empty");
        let cache = root.join("cache");
        write_pywal(&cache, "{}");
        let d = ThemeColors::default();

        let c = ThemeColors::load_from(None, None, Some(&cache));

        assert_eq!(c.accent, d.accent);
        assert_eq!(c.red, d.red);
        assert_eq!(c.orange, d.orange);
        assert_eq!(c.dim, d.dim);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pywal_orange_is_synthesized_between_yellow_and_red() {
        let c = ThemeColors::from_pywal(PYWAL).expect("valid pywal json");

        assert_eq!(c.orange, "#ec937b");
        assert_eq!(c.orange, blend_hex("#e0af68", "#f7768e", 0.5));
        assert_ne!(c.orange, c.red, "aliasing orange to red flattens the ramp");
        assert_ne!(c.orange, c.yellow);
    }

    #[test]
    fn a_pywal_accent_falls_back_to_the_cursor_color() {
        let c = ThemeColors::from_pywal(
            r##"{"special": {"cursor": "#bb9af7"}, "colors": {"color1": "#f7768e"}}"##,
        )
        .expect("valid pywal json");

        assert_eq!(c.accent, "#bb9af7");
        assert_eq!(c.border, "#bb9af7");
    }

    #[test]
    fn malformed_pywal_values_are_ignored() {
        let d = ThemeColors::default();

        let c = ThemeColors::from_pywal(
            r##"{"special": {"foreground": "wheat"}, "colors": {"color1": 12, "color2": "#9ece6a"}}"##,
        )
        .expect("valid pywal json");

        assert_eq!(c.text, d.text, "a non-hex name is not a color");
        assert_eq!(c.red, d.red, "a non-string value is not a color");
        assert_eq!(c.green, "#9ece6a", "the sound key still lands");
        assert_eq!(c.orange, d.orange, "no red/yellow pair to synthesize from");
    }

    #[test]
    fn xdg_base_dirs_win_over_the_home_defaults() {
        assert_eq!(
            xdg_home(Some("/run/user/1000/cache".into()), ".cache"),
            Some(PathBuf::from("/run/user/1000/cache")),
        );
        assert_eq!(
            xdg_home(Some("/xdg/state".into()), ".local/state"),
            Some(PathBuf::from("/xdg/state")),
        );

        let home = dirs::home_dir().expect("a home dir");
        assert_eq!(
            xdg_home(None, ".cache"),
            Some(home.join(".cache")),
            "unset falls back to ~/.cache"
        );
        assert_eq!(
            xdg_home(Some("".into()), ".local/state"),
            Some(home.join(".local/state")),
            "an empty var is treated as unset"
        );
    }

    #[test]
    fn named_keys_drive_every_color() {
        let c = ThemeColors::from_map(&map_of(NAMED));

        assert_eq!(c.accent, "#7aa2f7");
        assert_eq!(c.border, "#7aa2f7");
        assert_eq!(c.text, "#a9b1d6");
        assert_eq!(c.red, "#f7768e");
        assert_eq!(c.error, "#f7768e");
        assert_eq!(c.green, "#9ece6a");
        assert_eq!(c.yellow, "#e0af68");
        assert_eq!(c.orange, "#eb927b");
        assert_eq!(c.dim, blend_hex("#a9b1d6", "#1a1b26", 0.5));
    }

    #[test]
    fn numbered_keys_are_the_fallback_for_older_themes() {
        let c = ThemeColors::from_map(&map_of(LEGACY));

        assert_eq!(c.red, "#e06c75", "color1 stands in for red");
        assert_eq!(c.orange, "#e06c75", "and for orange, as it always did");
        assert_eq!(c.green, "#98c379", "color2");
        assert_eq!(c.yellow, "#e5c07b", "color3");
    }

    #[test]
    fn named_keys_win_over_numbered_ones() {
        let c = ThemeColors::from_map(&map_of(
            r##"
red = "#f7768e"
color1 = "#e06c75"
green = "#9ece6a"
color2 = "#98c379"
"##,
        ));

        assert_eq!(c.red, "#f7768e");
        assert_eq!(c.green, "#9ece6a");
    }

    #[test]
    fn one_missing_key_does_not_sink_the_whole_palette() {
        let d = ThemeColors::default();

        let c = ThemeColors::from_map(&map_of(r##"accent = "#7aa2f7""##));

        assert_eq!(c.accent, "#7aa2f7", "what the theme defines is honored");
        assert_eq!(c.red, d.red, "what it omits keeps the default");
        assert_eq!(c.green, d.green);
        assert_eq!(c.dim, d.dim, "dim needs both fg and bg");
    }

    #[test]
    fn a_theme_without_orange_borrows_its_red() {
        let c = ThemeColors::from_map(&map_of(
            r##"
red = "#f7768e"
green = "#9ece6a"
"##,
        ));

        assert_eq!(c.orange, "#f7768e");
    }

    #[test]
    fn every_accepted_hex_form_can_actually_be_blended() {
        // The guard accepts shorthand and alpha forms, so the blend maths must too —
        // otherwise `dim` silently falls back to the One Dark constants.
        for (short, long) in [("#abc", "#aabbcc"), ("#abcd", "#aabbcc")] {
            assert!(is_hex_color(short));
            assert_eq!(parse_hex(short), parse_hex(long), "{short}");
        }
        assert_eq!(
            parse_hex("#11223344"),
            parse_hex("#112233"),
            "alpha dropped"
        );
        assert_eq!(parse_hex("#zzzzzz"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("abcdef"), None, "the # is required");
    }

    #[test]
    fn a_shorthand_theme_derives_a_real_dim_instead_of_the_builtin_one() {
        let c = ThemeColors::from_map(&map_of(
            r##"
foreground = "#fff"
background = "#000"
"##,
        ));

        assert_eq!(c.text, "#fff");
        assert_eq!(c.dim, "#808080", "midpoint of white and black");
        assert_ne!(c.dim, ThemeColors::default().dim);
    }

    #[test]
    fn empty_or_malformed_values_are_ignored() {
        let d = ThemeColors::default();

        let c = ThemeColors::from_map(&map_of(
            r##"
accent = ""
red = "not-a-color"
green = "#12345"
yellow = "#zzzzzz"
"##,
        ));

        assert_eq!(c.accent, d.accent);
        assert_eq!(c.red, d.red);
        assert_eq!(c.green, d.green);
        assert_eq!(c.yellow, d.yellow);
    }
}
