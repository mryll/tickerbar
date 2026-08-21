// Binary-level never-crash invariant: bad input must still exit 0 with valid Waybar JSON.
use assert_cmd::Command;

const MISSING_CONFIG: &str = "/nonexistent/tickerbar-xyz.toml";

fn run(args: &[&str]) -> String {
    // NO_COLOR is removed so a developer who exports it globally still gets the colored
    // baseline these tests assert against; `run_with_env` sets it deliberately.
    let assert = Command::cargo_bin("tickerbar")
        .unwrap()
        .env_remove("NO_COLOR")
        .args(args)
        .assert()
        .success();
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

fn run_with_env(args: &[&str], key: &str, value: &str) -> String {
    let assert = Command::cargo_bin("tickerbar")
        .unwrap()
        .env(key, value)
        .args(args)
        .assert()
        .success();
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

fn tooltip_of(stdout: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON on stdout");
    v["tooltip"].as_str().unwrap_or_default().to_string()
}

fn assert_valid_waybar_json(stdout: &str) {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON on stdout");
    assert!(v.get("text").is_some(), "missing `text`");
    assert!(v.get("class").is_some(), "missing `class`");
}

#[test]
fn a_missing_config_file_still_emits_valid_waybar_json_and_exits_zero() {
    let out = run(&["--config", MISSING_CONFIG]);
    assert_valid_waybar_json(&out);
}

#[test]
fn an_unknown_flag_still_emits_valid_waybar_json_and_exits_zero() {
    let out = run(&["--definitely-not-a-flag"]);
    assert_valid_waybar_json(&out);
}

fn assert_valid_data_json(stdout: &str) {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON on stdout");
    assert_eq!(v["schema_version"], 1, "missing/wrong `schema_version`");
    assert!(v.get("state").is_some(), "missing `state`");
    assert!(v["groups"].is_array(), "missing `groups` array");
}

#[test]
fn json_mode_with_a_missing_config_still_emits_valid_structured_json_and_exits_zero() {
    let out = run(&["--output", "json", "--config", MISSING_CONFIG]);
    assert_valid_data_json(&out);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["state"], "error");
    assert!(v["error"]["message"].is_string());
}

#[test]
fn invalid_argv_in_json_mode_still_falls_back_to_structured_json() {
    // Clap parsing fails on the unknown flag, but the argv asked for structured output —
    // the exit-0 fallback must speak that format, not waybar.
    let out = run(&["--output", "json", "--definitely-not-a-flag"]);
    assert_valid_data_json(&out);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["state"], "error");
    assert!(v["error"]["message"].is_string());

    let eq_form = run(&["--output=json", "--definitely-not-a-flag"]);
    assert_valid_data_json(&eq_form);
}

#[test]
fn a_trailing_output_flag_without_a_value_falls_back_to_waybar_json() {
    let out = run(&["--definitely-not-a-flag", "--output"]);
    assert_valid_waybar_json(&out);
}

#[test]
fn an_invalid_output_format_still_emits_valid_json_and_exits_zero() {
    let out = run(&["--output", "nope"]);
    assert_valid_waybar_json(&out);
}

#[test]
fn an_unknown_no_color_value_is_an_argument_error_that_still_emits_valid_json() {
    assert_valid_waybar_json(&run(&["--no-color=not-a-surface"]));
    assert_valid_data_json(&run(&["--output", "json", "--no-color=not-a-surface"]));
}

#[test]
fn no_color_strips_the_tint_from_the_error_tooltip_too() {
    let colored = tooltip_of(&run(&["--config", MISSING_CONFIG]));
    assert!(colored.contains("foreground="), "baseline is colored");

    for args in [
        vec!["--config", MISSING_CONFIG, "--no-color"],
        vec!["--config", MISSING_CONFIG, "--no-color=all"],
        vec!["--config", MISSING_CONFIG, "--no-color=tooltip"],
    ] {
        let plain = tooltip_of(&run(&args));
        assert!(!plain.contains("foreground="), "{args:?} left a tint");
        assert!(!plain.contains('#'), "{args:?} left inline hex");
    }
}

#[test]
fn the_no_color_environment_variable_reaches_the_rendered_output() {
    let plain = tooltip_of(&run_with_env(
        &["--config", MISSING_CONFIG],
        "NO_COLOR",
        "1",
    ));
    assert!(!plain.contains("foreground="));

    // Explicitly empty means "not set", per no-color.org.
    let colored = tooltip_of(&run_with_env(&["--config", MISSING_CONFIG], "NO_COLOR", ""));
    assert!(colored.contains("foreground="));
}

#[test]
fn an_explicit_scope_beats_the_environment_variable_end_to_end() {
    // NO_COLOR set, but `--no-color=bar` scopes the request to the bar only, so the
    // tooltip must still be colored.
    let out = run_with_env(
        &["--config", MISSING_CONFIG, "--no-color=bar"],
        "NO_COLOR",
        "1",
    );
    assert!(tooltip_of(&out).contains("foreground="));
}

#[test]
fn the_structured_json_is_unaffected_by_no_color() {
    // Only the run timestamp may legitimately differ between two invocations.
    let normalize = |s: &str| {
        let mut v: serde_json::Value = serde_json::from_str(s.trim()).unwrap();
        v["fetched_at"] = serde_json::Value::String(String::new());
        serde_json::to_string(&v).unwrap()
    };
    let colored = run(&["--output", "json", "--config", MISSING_CONFIG]);
    let plain = run(&["--output", "json", "--config", MISSING_CONFIG, "--no-color"]);

    assert_eq!(normalize(&colored), normalize(&plain));
    assert!(!plain.contains("foreground="));
}

#[test]
fn the_structured_document_always_publishes_a_real_palette() {
    // Frontends paint from this, so it must be present even on the error path and must
    // never go monochrome — `--no-color` is about the Pango surfaces only.
    for args in [
        vec!["--output", "json", "--config", MISSING_CONFIG],
        vec!["--output", "json", "--config", MISSING_CONFIG, "--no-color"],
    ] {
        let v: serde_json::Value = serde_json::from_str(run(&args).trim()).unwrap();
        let p = &v["palette"];
        for key in ["up", "down", "flat", "text", "dim", "accent", "error"] {
            let c = p[key].as_str().unwrap_or_default();
            assert!(
                c.starts_with('#') && c.len() >= 4,
                "{args:?}: palette.{key} = {c:?}"
            );
        }
        assert_ne!(
            p["up"], p["down"],
            "direction colours must be tellable apart"
        );
    }
}
