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
