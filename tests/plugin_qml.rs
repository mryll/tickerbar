// Static contract of the not-installed discrimination and the copy-install
// button in the Omarchy panel. The QML has no test runner; these substring
// checks pin the load-bearing lines. Expectations are written out by hand.
static PANEL: &str = include_str!("../omarchy/TickerPanel.qml");

#[test]
fn a_run_is_marked_not_installed_only_without_an_exit_signal() {
    assert!(PANEL.contains("sawExit = false"), "startRun must reset sawExit");
    assert!(PANEL.contains("root.sawExit = true"), "onExited must set sawExit");
    assert!(
        PANEL.contains("} else if (!sawExit || exitCode === 126 || exitCode === 127) {"),
        "gate on !sawExit or sh's exec-failure codes"
    );
    assert!(
        PANEL.contains(r#"["/bin/sh", "-c", 'exec "$0" "$@"'].concat(cmd)"#),
        "the command must be wrapped in sh (claudebar#6: a missing binary can abort the shell)"
    );
    assert!(
        !PANEL.contains("command = cmd"),
        "the direct (unwrapped) command assignment is banned"
    );
    assert!(
        PANEL.contains("tripwireFired = false") && PANEL.contains("if (tripwireFired) {"),
        "the empty branch gates on the per-run tripwire flag, not stale text"
    );
    assert!(
        PANEL.contains("produced no output (exit "),
        "a run that exited empty is an operational error"
    );
}

#[test]
fn the_install_command_is_one_constant_copied_as_argv() {
    assert_eq!(
        PANEL.matches("yay -S tickerbar-bin").count(),
        1,
        "installCmd literal must appear exactly once"
    );
    assert!(
        PANEL.contains(r#"Util.execArgv(["wl-copy", root.installCmd])"#),
        "copy must go through execArgv argv-style"
    );
    assert!(!PANEL.contains("bash -c"), "no shell line around wl-copy");
    assert!(
        PANEL.contains("visible: root.notInstalled"),
        "the button gates on notInstalled — topError also carries CLI errors"
    );
}

// tickerbar#1: since omarchy 1702cf0 the host injects a PluginBarApi facade
// whose `centerHoverRevealSuppressed` is read-only. Assigning it throws, and
// a throw inside close() before hide() strands a full-screen overlay.
#[test]
fn the_panel_hides_before_touching_the_bar_hover_state() {
    let close = PANEL
        .split("function close() {")
        .nth(1)
        .expect("close() must exist")
        .split('}')
        .next()
        .unwrap();
    let hide = close
        .find("root.controller.hide()")
        .expect("close() must hide");
    let setter = close
        .find("setCenterHoverRevealSuppressed(false)")
        .expect("close() must release the hover suppression");
    assert!(hide < setter, "hide() must run before the bar setter");
}

#[test]
fn the_hover_suppression_goes_through_the_plugin_bar_api_setter() {
    assert!(
        PANEL.contains(r#"typeof root.bar.setCenterHoverRevealSuppressed === "function""#),
        "must prefer PluginBarApi.setCenterHoverRevealSuppressed()"
    );
    // Only the setter body: the panel has other try blocks further down.
    let setter_fn = PANEL
        .split("function setCenterHoverRevealSuppressed(value) {")
        .nth(1)
        .expect("setter must exist")
        .split("\n  }\n")
        .next()
        .unwrap();
    let call = setter_fn
        .find("root.bar.setCenterHoverRevealSuppressed(value)")
        .expect("the setter must call the PluginBarApi function");
    let assign = setter_fn
        .find("root.bar.centerHoverRevealSuppressed = value")
        .expect("the setter must keep the assignment for hosts that inject the real Bar");
    assert!(
        call < assign,
        "the function call comes first, the assignment is the fallback"
    );
    assert!(
        setter_fn.contains("try {") && setter_fn.contains("} catch (e) {"),
        "a throw from the host API must not escape"
    );
    assert!(
        !setter_fn.contains("throw"),
        "the catch must swallow, not rethrow"
    );
}
