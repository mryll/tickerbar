// Binary-level never-crash invariant: bad input must still exit 0 with valid Waybar JSON.
use assert_cmd::Command;

fn run(args: &[&str]) -> String {
    let assert = Command::cargo_bin("tickerbar")
        .unwrap()
        .args(args)
        .assert()
        .success();
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

fn assert_valid_waybar_json(stdout: &str) {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON on stdout");
    assert!(v.get("text").is_some(), "missing `text`");
    assert!(v.get("class").is_some(), "missing `class`");
}

#[test]
fn a_missing_config_file_still_emits_valid_waybar_json_and_exits_zero() {
    let out = run(&["--config", "/nonexistent/tickerbar-xyz.toml"]);
    assert_valid_waybar_json(&out);
}

#[test]
fn an_unknown_flag_still_emits_valid_waybar_json_and_exits_zero() {
    let out = run(&["--definitely-not-a-flag"]);
    assert_valid_waybar_json(&out);
}
