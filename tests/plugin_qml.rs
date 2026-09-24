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
// tickerbar#2: the strip sized itself against Bar internals (`moduleSlots`,
// `slotWindow`, `centerAnchor`, ...) that the PluginBarApi facade does not
// carry, so its guard failed on every evaluation and it never self-limited.
// The rewrite measures the slot Items of THIS window's tree and never reads
// its own width; when the tree is not recognized it caps to `stripMaxPercent`.
static WIDGET: &str = include_str!("../omarchy/BarWidget.qml");
static MANIFEST: &str = include_str!("../manifest.json");

fn strip_max_width() -> &'static str {
    WIDGET
        .split("readonly property real stripMaxWidth: {")
        .nth(1)
        .expect("stripMaxWidth must exist")
        .split("\n  }\n")
        .next()
        .unwrap()
}

#[test]
fn the_strip_measures_the_bars_slot_items_not_bar_internals() {
    for gone in [
        "bar.moduleSlots",
        "bar.slotWindow",
        "bar.sameWindow",
        "bar.layoutEntries",
        "bar.centerAnchor",
        "barHost.",
    ] {
        assert!(
            !WIDGET.contains(gone),
            "{gone} does not exist on the PluginBarApi facade"
        );
    }
    assert!(
        WIDGET
            .contains(r#"typeof item.region === "string" && typeof item.moduleName === "string""#),
        "neighbors are the bar's slot Items, recognized by their contract"
    );
    let body = strip_max_width();
    assert!(
        body.contains("while (top.parent) top = top.parent"),
        "the walk is scoped to this window's tree (one bar per monitor)"
    );
    assert!(
        body.contains("collectSlots(top, all, { n: 4000 })"),
        "the walk is bounded"
    );
}

#[test]
fn render_fully_is_reserved_for_a_bar_that_is_not_sized_yet() {
    let body = strip_max_width();
    assert_eq!(
        body.matches("return -1").count(),
        2,
        "-1 only for no bar / vertical and for an unsized window"
    );
    let guard = body
        .find("if (!root.bar || root.vertical) return -1")
        .unwrap();
    let win = body
        .find("if (!win || !(win.width > 0)) return -1")
        .unwrap();
    let blind = body
        .find("var blind = Math.max(0, W * root.stripMaxFraction - margin - safety)")
        .unwrap();
    let walk = body.find("var mine = hostSlot()").unwrap();
    let slot = body.find("if (!mine) return blind").unwrap();
    assert!(
        guard < win && win < blind && blind < walk && walk < slot,
        "guards run before any tree walk"
    );
    assert!(
        body.contains("if (mine.region !== \"center\") return blind"),
        "an unknown region caps blindly instead of rendering fully"
    );
}

#[test]
fn the_geometry_never_reads_this_widgets_own_width() {
    let body = strip_max_width();
    assert!(
        body.contains("if (s === mine || s.moduleName === \"\") continue"),
        "self is skipped before any width is read"
    );
    let skip = body
        .find("if (s === mine || s.moduleName === \"\") continue")
        .unwrap();
    let width = body.find("!(s.width > 0)").unwrap();
    assert!(skip < width, "self is skipped before any width is read");
    for own in [
        "mine.width",
        "root.width",
        "root.implicitWidth",
        "button.",
        "contentRow.",
    ] {
        assert!(
            !body.contains(own),
            "reading {own} would close a binding loop through fitCount"
        );
    }
}

#[test]
fn an_anchored_center_budgets_one_flank_and_the_tighter_side_when_the_anchor_is_us() {
    let body = strip_max_width();
    assert!(
        body.contains("if (l <= half && l + center[k].width >= half) anchor = center[k]"),
        "the anchor is the slot that straddles the middle"
    );
    assert!(
        body.contains("if (s.region === \"center\" && s.parent !== mine.parent) oneRow = false"),
        "a plain center row is told apart from flanks by the shared parent"
    );
    let skip = body
        .find("if (s === mine || s.moduleName === \"\") continue")
        .unwrap();
    let structure = body.find("oneRow = false").unwrap();
    let visible = body
        .find("if (!s.visible || !(s.width > 0)) continue")
        .unwrap();
    assert!(
        structure < visible,
        "structure is decided before hidden slots are dropped: a hidden anchor still pins the flanks"
    );
    assert!(
        body.contains("s.moduleName === \"\"") && skip < structure,
        "the always-built empty anchor slot (no anchor configured) is not structure"
    );
    assert!(
        body.contains("Math.min(half - leftBound - before, rightBound - half - after) - safety"),
        "with no other straddler the tighter side bounds us"
    );
    assert!(body.contains("Math.max(0, rightBound - aRight - after - safety)"));
    assert!(body.contains("Math.max(0, aLeft - leftBound - before - safety)"));
}

#[test]
fn the_blind_cap_is_the_stripmaxpercent_setting() {
    let m: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(m["barWidget"]["defaults"]["stripMaxPercent"], 30);
    let field = m["barWidget"]["schema"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "stripMaxPercent")
        .expect("the schema declares stripMaxPercent");
    assert_eq!(field["type"], "integer");
    assert_eq!(field["defaultValue"], 30);
    assert_eq!(field["min"], 10);
    assert_eq!(field["max"], 90);
    assert_eq!(field["step"], 5);
    assert!(WIDGET.contains("root.settings ? root.settings.stripMaxPercent : undefined"));
    assert!(
        WIDGET.contains("return v >= 10 && v <= 90 ? v / 100 : 0.30"),
        "out-of-range or missing values fall back to the measured 30%"
    );
}

#[test]
fn a_trimmed_strip_budgets_the_marker_and_paints_it_only_beside_an_entry() {
    let fit = WIDGET
        .split("readonly property int fitCount: {")
        .nth(1)
        .expect("fitCount must exist")
        .split("\n  }\n")
        .next()
        .unwrap();
    assert!(
        fit.contains("if (max < 0) return n"),
        "no geometry yet: render fully"
    );
    assert!(
        fit.contains("if (need + (i < n - 1 ? markerW : 0) > max) break"),
        "every non-final entry reserves room for the … marker"
    );
    assert!(
        WIDGET.contains("if (shown > 0 && truncated)"),
        "the marker is painted only next to at least one entry"
    );
}

// The binding's JS, executed. tests/fixtures/strip_budget_harness.js pulls
// the helper functions and the stripMaxWidth body out of the QML and runs
// them against item trees shaped like Bar.qml's. Expected budgets are worked
// out by hand from the host's layout: 8px section margins, 12px safety gap.
// Every tree also carries the anchor slot the bar always builds (empty and
// hidden when no anchor is configured), and the harness itself fails when a
// budget moves with the widget's own width (the binding-loop tripwire).
// Skipped when node is not installed (the QML has no other runtime here).
#[test]
fn the_budget_matches_the_hosts_layout_on_synthetic_bars() {
    let output = match std::process::Command::new("node")
        .arg("tests/fixtures/strip_budget_harness.js")
        .arg("omarchy/BarWidget.qml")
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("node not installed: skipping the executed-budget check");
            return;
        }
        Err(e) => panic!("could not run node: {e}"),
    };
    assert!(
        output.status.success(),
        "harness failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let got: std::collections::HashMap<String, f64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| {
            let (k, v) = l.split_once('\t').expect("name<TAB>value");
            (k.to_string(), v.parse().expect("a number"))
        })
        .collect();
    // (scenario, budget) — W = 1536 unless noted; see the harness for the trees.
    let expected: [(&str, f64); 12] = [
        // after-flank: right section starts at 1536-8-210 = 1318, flank starts
        // at 768+60 = 828, other flank slots 70, gap 12.
        ("after_flank", 408.0),
        // before-flank: flank ends at 708, left section ends at 208, other 60.
        ("before_flank", 428.0),
        // centered row: 2*min(768-308, 1428-768) - 150 - 12
        ("plain_center_row", 758.0),
        ("alone_in_center", 908.0),
        // we are the anchor, right side tighter: min(768-18-30, 928-768-40) - 12
        ("me_as_anchor_right_tighter", 108.0),
        // hidden anchor: the flank starts at the middle; tighter side once
        ("hidden_anchor_two_flanks", 648.0),
        // W = 1000, tray from x = 900, hidden anchor, only our row visible:
        // 900 - 500 - 12, NOT the centered 2*(900-500)-12 = 788.
        ("hidden_anchor_own_row_only", 388.0),
        // right section: 1528 - 90 - (768+60+40) - 12
        ("right_region", 558.0),
        // left section: center block starts at 768-60-60 = 648; 648-8-50-12
        ("left_region", 578.0),
        ("left_region_empty_center", 1408.0),
        // no slot ancestor: 1536*0.30 - 8 - 12
        ("blind_no_slot", 440.8),
        ("blind_setting_50", 748.0),
    ];
    for (name, want) in expected {
        let have = got
            .get(name)
            .unwrap_or_else(|| panic!("harness printed no {name}"));
        assert!(
            (have - want).abs() < 1e-6,
            "{name}: got {have}, want {want}"
        );
    }
    assert_eq!(
        got.len(),
        expected.len(),
        "unexpected scenarios in the harness output"
    );
}
