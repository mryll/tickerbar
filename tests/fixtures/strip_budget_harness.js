// Runs the real stripMaxWidth body from omarchy/BarWidget.qml against
// synthetic item trees shaped like omarchy's Bar.qml (sections at the edges
// with an 8px margin, the center either one centered row or two flanks
// around an anchor slot centered on the bar). Usage:
//   node strip_budget_harness.js path/to/BarWidget.qml
// Prints one "name<TAB>budget" line per scenario; tests/plugin_qml.rs owns
// the expected numbers. Only the JS of the binding runs here: no QML engine,
// so binding loops and re-evaluation order are out of scope.
"use strict"
const fs = require("fs")
const src = fs.readFileSync(process.argv[2], "utf8")

function fn(name) {
  const m = src.match(new RegExp("\\n  function " + name + "\\([^)]*\\) \\{\\n[\\s\\S]*?\\n  \\}\\n"))
  if (!m) throw new Error("function " + name + " not found")
  return m[0].replace("  function ", "function ")
}
const body = src.split("readonly property real stripMaxWidth: {")[1].split("\n  }\n")[0]

const Style = { space: n => n }
const root = { bar: {}, vertical: false, settings: {}, stripMaxFraction: 0.30, parent: null,
  QsWindow: { window: { width: 0 } } }
// eslint-disable-next-line no-new-func
const compute = new Function("Style", "root",
  ["isModuleSlot", "hostSlot", "collectSlots", "leftIn", "sumWidths"].map(fn).join("\n")
  + "\nreturn function stripMaxWidth() {" + body + "\n}")(Style, root)

function Item(p) {
  p = p || {}
  this.x = p.x || 0; this.width = p.width || 0; this.visible = p.visible !== false
  this.parent = null; this.children = []
  if (p.region !== undefined) { this.region = p.region; this.moduleName = p.moduleName !== undefined ? p.moduleName : "m" }
}
Item.prototype.add = function (c) { c.parent = this; this.children.push(c); return c }

// o: { left:[w], right:[w], row:[w] | anchor:w, before:[w], after:[w],
//      me: "left"|"right"|"row"|"before"|"after"|"anchor", meIndex }
// A width of 0 in `me`'s slot never matters: the widget's own slot is skipped.
let build = function (W, o) {
  root.QsWindow.window.width = W
  root.parent = null
  const top = new Item({ width: W })
  const sections = top.add(new Item({ width: W }))
  function row(parentItem, widths, xOf, region, meIdx) {
    const loader = parentItem.add(new Item({}))
    const r = loader.add(new Item({}))
    let x = 0, total = 0
    widths.forEach((w, i) => {
      const s = r.add(new Item({ x, width: w, region, moduleName: region + i }))
      if (i === meIdx) { const l = s.add(new Item({ width: w })); l.add(root); root.parent = l }
      x += w; total += w
    })
    r.width = total; loader.width = total; loader.x = xOf(total)
  }
  const lw = o.left || [], rw = o.right || []
  row(sections, lw, () => 8, "left", o.me === "left" ? o.meIndex : -1)
  row(sections, rw, t => W - 8 - t, "right", o.me === "right" ? o.meIndex : -1)
  const centerItem = sections.add(new Item({ width: W })).add(new Item({ width: W }))
  if (o.row) {
    // Bar.qml always builds the anchor slot; with no anchor configured it
    // stays hidden, zero-wide, with an empty moduleName, beside the row.
    centerItem.add(new Item({ x: W / 2, width: 0, region: "center", moduleName: "", visible: false }))
    row(centerItem, o.row, t => (W - t) / 2, "center", o.me === "row" ? o.meIndex : -1)
  } else {
    const aw = o.anchor || 0
    const anchor = centerItem.add(new Item({ x: (W - aw) / 2, width: aw, region: "center", moduleName: "clock", visible: aw > 0 }))
    if (o.me === "anchor") { const l = anchor.add(new Item({ width: aw })); l.add(root); root.parent = l }
    row(centerItem, o.before || [], t => (W - aw) / 2 - t, "center", o.me === "before" ? o.meIndex : -1)
    row(centerItem, o.after || [], () => (W + aw) / 2, "center", o.me === "after" ? o.meIndex : -1)
  }
  if (!root.parent) throw new Error("scenario did not place the widget")
  return compute()
}

const W = 1536
const scenarios = {
  // probook: [indicators | clock | keyboard, update, tickerbar], right = 6 icons
  after_flank: () => build(W, { anchor: 120, before: [60], after: [40, 30, 500], me: "after", meIndex: 2, right: [60, 30, 30, 30, 30, 30], left: [200] }),
  before_flank: () => build(W, { anchor: 120, before: [500, 60], after: [40], me: "before", meIndex: 0, right: [210], left: [200] }),
  plain_center_row: () => build(W, { row: [100, 500, 50], me: "row", meIndex: 1, left: [300], right: [100] }),
  alone_in_center: () => build(W, { row: [500], me: "row", meIndex: 0, left: [300], right: [100] }),
  me_as_anchor_right_tighter: () => build(W, { anchor: 500, before: [30], after: [40], me: "anchor", left: [10], right: [600] }),
  hidden_anchor_two_flanks: () => build(W, { anchor: 0, before: [30], after: [500], me: "after", meIndex: 0, left: [10], right: [100] }),
  // Codex r1 P1: hidden anchor, every visible center slot in our own row
  hidden_anchor_own_row_only: () => build(1000, { anchor: 0, after: [600], me: "after", meIndex: 0, right: [92] }),
  right_region: () => build(W, { anchor: 120, before: [60], after: [40], me: "right", meIndex: 1, right: [60, 500, 30], left: [200] }),
  left_region: () => build(W, { anchor: 120, before: [60], after: [40], me: "left", meIndex: 0, right: [100], left: [500, 50] }),
  left_region_empty_center: () => build(W, { row: [], me: "left", meIndex: 0, right: [100], left: [500] }),
  blind_no_slot: () => { root.QsWindow.window.width = W; root.parent = new Item({ width: 100 }); return compute() },
  blind_setting_50: () => { root.QsWindow.window.width = W; root.parent = new Item({ width: 100 }); root.stripMaxFraction = 0.5; return compute() },
}
// The widget's own width must never move the budget (it would close a
// binding loop through fitCount). Each scenario is built with the declared
// own width and again 400px wider; a difference fails the whole harness.
let ownWidth = 0
const buildScenario = build
function buildWithOwnWidth(W, o) {
  const grow = list => list.map((w, i) => (i === o.meIndex ? w + ownWidth : w))
  const p = Object.assign({}, o)
  for (const key of ["left", "right", "row", "before", "after"]) {
    if (o[key] && o.me === key) p[key] = grow(o[key])
  }
  if (o.me === "anchor") p.anchor = o.anchor + ownWidth
  return buildScenario(W, p)
}
build = buildWithOwnWidth
for (const name of Object.keys(scenarios)) {
  ownWidth = 0
  const a = scenarios[name]()
  ownWidth = 400
  const b = scenarios[name]()
  if (a !== b) throw new Error(name + ": budget follows the widget's own width (" + a + " vs " + b + ")")
  console.log(name + "\t" + a)
}
