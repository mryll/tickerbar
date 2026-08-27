pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Quickshell.Io
import qs.Commons
import qs.Ui

// Market table popup + data owner. This panel runs `tickerbar --output json`
// (the structured raw-data mode) on a timer and exposes the parsed snapshot;
// BarWidget.qml renders the compact bar strip from `barEntries`.
Panel {
  id: root
  moduleName: "mryll.tickerbar"
  ipcTarget: "mryll.tickerbar"
  manageIpc: false

  property var anchorItem: null
  property bool openedFromHotkey: false

  // The bar identifies this panel by the widget mounted in its slot, not by
  // this nested item — same contract as the first-party weather plugin.
  property var hostWidget: null
  readonly property var barIdentity: hostWidget || root

  // Last successfully parsed `tickerbar --output json` document. Kept on
  // fetch/parse failure so stale data stays visible instead of flashing empty.
  property var snapshot: null

  // Run-level failure (binary missing, malformed output). Cleared by the next
  // good document; last-known-good data stays rendered under the error banner.
  property string runError: ""

  // The panel draws on the POPUP CARD, so it takes the popup surface's text
  // token — not the bar's. bar.foreground is chosen against the bar, which on a
  // transparent bar means "against the wallpaper"; that is the wrong contrast
  // reference for a card, and a theme that defines popups.text separately would
  // be ignored outright. (printbar already did this; the rest of the family now
  // agrees.)
  readonly property color panelFg: Color.popups.text
  readonly property color mutedFg: Qt.darker(panelFg, 1.55)

  // ---- Freshness suffix tint, shared by the whole family. The timestamp is
  // ALWAYS dim ("when is this from" is information, not a warning); only the
  // "· stale" / "· partial data" suffix carries a muted warning tone.
  readonly property color freshnessWarn: panelColored
    ? mixColor(mutedFg, urgentColor, 0.4) : mutedFg
  readonly property color urgentColor: root.bar ? root.bar.urgent : Color.urgent
  readonly property string panelFont: root.bar ? root.bar.fontFamily : Style.font.family

  readonly property int refreshSeconds: Math.max(5, parseInt(setting("refreshIntervalSec", 60), 10) || 60)

  // Tri-state summary: "follow" honors the TOML's summary_format presence,
  // "show"/"hide" force it either way. It governs the BAR STRIP only — see
  // summaryChipVisible for the panel, which shows the average by default.
  readonly property string summaryMode: String(setting("summaryMode", "follow"))
  readonly property bool showSummary: summaryMode === "show"
    || (summaryMode !== "hide" && snapshot !== null && snapshot.summary_configured === true)

  // Monochrome, mirroring the CLI's `--no-color`: "full" (default), "none",
  // "bar-only" (colored bar face, monochrome panel) and "panel-only" (the
  // reverse). Dimming and the muted closed/stale treatments are NOT color, so
  // they survive every mode; only accent/urgent tinting is gated.
  // An unrecognized value normalizes to "full": a hand-edited shell.json must
  // not be able to silently take the color off both surfaces.
  readonly property string colorMode: {
    var v = String(setting("colorMode", "full"))
    return ["full", "none", "bar-only", "panel-only"].indexOf(v) >= 0 ? v : "full"
  }
  readonly property bool barColored:   colorMode === "full" || colorMode === "bar-only"
  readonly property bool panelColored: colorMode === "full" || colorMode === "panel-only"

  readonly property string binName: "tickerbar"
  readonly property string configPath: String(setting("configPath", "")).trim()

  // Measurements sent with the last run, to detect when fresh content moves
  // the fit and one cache-hot repack run is warranted.
  property int lastSentBudget: 0
  property int lastSentMaxCols: 0

  function buildCmd() {
    var args = [binName, "--output", "json"]
    if (configPath !== "") { args.push("--config"); args.push(configPath) }
    // Presentation measurements only — the CORE merges them with the config
    // (an explicit tooltip_rows_per_column > 0 wins; max-columns takes the
    // smaller positive value) and does ALL the packing.
    lastSentBudget = autoLineBudget
    lastSentMaxCols = maxFitColumns
    args.push("--rows-per-column"); args.push(String(lastSentBudget))
    args.push("--max-columns"); args.push(String(lastSentMaxCols))
    return args
  }

  // A settings change mid-poll must not be dropped: startRun queues the new
  // command (last-command-wins) while a run is in flight.
  onConfigPathChanged: startRun(buildCmd())

  readonly property var groups: snapshot && snapshot.groups ? snapshot.groups : []
  // A failed plugin run counts as stale even when the cached snapshot still
  // says "ok" — the prices on screen are the last good ones either way.
  readonly property string lifecycle: pluginStale
    ? "stale" : (snapshot ? String(snapshot.state || "") : "")
  readonly property string topError: runError !== "" ? runError
    : (snapshot && snapshot.error && snapshot.error.message ? String(snapshot.error.message) : "")

  readonly property string updatedText: {
    if (!snapshot || !snapshot.fetched_at) return ""
    var d = new Date(snapshot.fetched_at)
    return isNaN(d.getTime()) ? "" : Qt.formatTime(d, "HH:mm")
  }

  // ---- Formatting (mirrors the Rust renderer: grouped thousands, 2/4 decimals,
  //      rates as percents). Numbers arrive raw; all presentation happens here.
  function fmtNumber(v) {
    var a = Math.abs(v)
    if (a >= 1000) return Number(v).toLocaleString(Qt.locale("en_US"), 'f', 2)
    if (a >= 1) return Number(v).toFixed(2)
    return Number(v).toFixed(4)
  }

  function priceText(r) {
    if (r.price === null || r.price === undefined) return "—"
    if (r.unit === "percent") return Number(r.price).toFixed(2) + "%"
    return fmtNumber(r.price)
  }

  // Mirrors the CLI's fmt_change: signed, EXCEPT a flat quote, which renders
  // unsigned with a leading space so the right-aligned column keeps its width.
  // Flatness comes from the document's `direction`, not from the rounded number,
  // so a tiny-but-nonzero move that prints as 0.00 keeps its real sign.
  function changeText(c, dir) {
    if (c === null || c === undefined) return ""
    if (dir === "flat") return " " + Math.abs(Number(c)).toFixed(2) + "%"
    return (c >= 0 ? "+" : "") + Number(c).toFixed(2) + "%"
  }

  // Direction is already double-encoded (sign + tint); a triangle on top of
  // that was redundant, so the change cell is just the signed percent.
  function chgCellText(r) {
    return changeText(r.change_pct, r.direction)
  }

  function rangeText(r) {
    if (r.day_low === null || r.day_low === undefined) return ""
    if (r.day_high === null || r.day_high === undefined) return ""
    if (r.unit === "percent")
      return Number(r.day_low).toFixed(2) + "%–" + Number(r.day_high).toFixed(2) + "%"
    return fmtNumber(r.day_low) + "–" + fmtNumber(r.day_high)
  }

  // `groupClosed` suppresses the per-row closed marker when the whole section
  // is closed — the header already says it once; repeating it on sixty rows
  // is noise, not information.
  function noteText(r, groupClosed) {
    var parts = []
    var range = rangeText(r)
    if (range !== "") parts.push(range)
    if (r.market === "closed" && groupClosed !== true) parts.push(" closed")
    if (r.state === "stale") parts.push("stale")
    else if (r.state === "error")
      parts.push(r.error && r.error.message ? String(r.error.message) : "n/d")
    else if (r.state === "missing" && groupClosed !== true)
      // Missing data in a fully-closed section is expected (no session cache
      // yet) — the header's pause marker already explains it; per-row "no
      // data" only matters while the market is open.
      parts.push(r.error && r.error.message ? String(r.error.message) : "n/d")
    return parts.join("  ")
  }

  // ---- Direction tinting: interpolate from the theme foreground toward the
  //      theme accent (up) / urgent (down), so every Omarchy theme works.
  //      No hardcoded green/red.
  function mixColor(a, b, t) {
    return Qt.rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, 1)
  }

  // ---- The CORE publishes the direction colours it paints with (`palette.up` /
  //      `.down` / `.flat`, the same values its Waybar renderer uses), so the panel,
  //      the bar strip and the Waybar tooltip agree on what a given number looks
  //      like. Re-deriving them here from the accent is what made `+2.14%` render
  //      accent-blue in the panel and theme-green everywhere else.
  readonly property var palette: snapshot && snapshot.palette ? snapshot.palette : null

  function paletteColor(key, fallback) {
    if (!palette) return fallback
    var v = palette[key]
    return (typeof v === "string" && v.charAt(0) === "#") ? v : fallback
  }

  // Where a direction wants to land. Documents from a pre-`palette` binary keep the
  // old theme-derived approximation so an older CLI still paints something sane.
  function dirTarget(dir, base) {
    if (palette) {
      if (dir === "up") return paletteColor("up", base)
      if (dir === "down") return paletteColor("down", base)
      return base
    }
    if (dir === "up") return Color.accent
    if (dir === "down") return urgentColor
    return base
  }

  // Raw tint. BarWidget calls this directly for the strip, which has its own mono
  // setting, so the gating lives at each surface instead of in here. `strength`
  // defaults to landing ON the core's colour; the few chrome spots that want a
  // muted variant pass their own.
  function dirColorFor(dir, base, strength) {
    var b = base === undefined ? panelFg : base
    var target = dirTarget(dir, b)
    if (target === b) return b
    var t = strength === undefined ? (palette ? 1.0 : 0.62) : strength
    return t >= 1.0 ? target : mixColor(b, target, t)
  }

  // Panel-surface tint: foreground only when the panel is monochrome. Direction
  // stays readable through the signed change text, exactly like the CLI tooltip.
  function panelDirColor(dir, base, strength) {
    var b = base === undefined ? panelFg : base
    return panelColored ? dirColorFor(dir, b, strength) : b
  }

  // Accent/urgent carriers, resolved once so the mono branch is in one place. Both
  // prefer the core's palette, falling back to the shell theme for older documents.
  readonly property color panelAccent: panelColored ? paletteColor("accent", Color.accent) : panelFg
  readonly property color panelUrgent: panelColored ? paletteColor("error", urgentColor) : panelFg

  // ---- Compact bar strip model consumed by BarWidget.qml: the CLI-resolved
  //      curated subset (config `display.bar`, order preserved), optionally
  //      preceded by the watchlist-average summary.
  readonly property var barEntries: {
    if (!snapshot) return []
    var out = []
    var avg = snapshot.avg_change_pct
    if (showSummary && avg !== null && avg !== undefined) {
      var avgDir = avg > 0 ? "up" : (avg < 0 ? "down" : "flat")
      // The average is not an asset, so it carries no class mark — the sigma IS
      // its mark.
      out.push({ glyph: "", label: "Σ", value: changeText(avg, avgDir), dir: avgDir, muted: false })
    }
    // snapshot.bar carries the same raw row objects as the grouped table (already
    // selected and ordered by the CLI), so duplicate labels stay unambiguous.
    var barRows = snapshot.bar || []
    for (var i = 0; i < barRows.length; i++) {
      var r = barRows[i]
      var chg = changeText(r.change_pct, r.direction)
      out.push({
        // The class mark comes from the CLI (row.glyph, the same one the group
        // header carries), so the strip never re-derives which section an asset
        // belongs to. Empty string when the payload predates the field.
        glyph: String(r.glyph || ""),
        label: String(r.label),
        value: priceText(r) + (chg !== "" ? " " + chg : ""),
        dir: r.direction,
        muted: r.market === "closed"
      })
    }
    return out
  }

  // ---- Shared column widths, measured across ALL rows so every group's grid
  //      lines up like one table (the QML analogue of the tooltip's global
  //      column widths). Small buffer absorbs metric rounding.
  FontMetrics {
    id: fm
    font.family: root.panelFont
    font.pixelSize: Style.font.body
  }

  // ---- Currency per group: most groups are single-currency, so the code
  //      lives in the section header ("· ARS", muted) — repeating it on every
  //      row would be noise. Only a genuinely mixed group (crypto can quote
  //      usd and ars per asset) gets a minimal per-row code left of the price.
  function groupCurrencyInfo(g) {
    var known = []
    var rows = g && g.rows ? g.rows : []
    for (var i = 0; i < rows.length; i++) {
      var r = rows[i]
      if (r.unit !== "currency") continue
      if (r.quote === null || r.quote === undefined || r.quote === "") continue
      var q = String(r.quote).toLowerCase()
      if (known.indexOf(q) === -1) known.push(q)
    }
    return { code: known.length === 1 ? known[0] : "", mixed: known.length > 1 }
  }

  // Any group mixing currencies right now? Gates the per-row currency column.
  readonly property bool anyMixedCurrency: {
    for (var i = 0; i < groups.length; i++) {
      if (groupCurrencyInfo(groups[i]).mixed) return true
    }
    return false
  }

  // Per-row currency cue, only inside mixed groups.
  function curCellText(r, groupMixed) {
    if (groupMixed !== true || r.unit !== "currency") return ""
    if (r.quote === null || r.quote === undefined || r.quote === "") return ""
    return String(r.quote).toLowerCase()
  }

  function columnWidth(kind) {
    var w = 0
    void fm.font.pixelSize // re-measure when the font scale changes
    for (var i = 0; i < groups.length; i++) {
      var g = groups[i]
      var mixed = groupCurrencyInfo(g).mixed
      var rows = g.rows || []
      for (var j = 0; j < rows.length; j++) {
        var r = rows[j]
        var text = kind === "label" ? String(r.label)
          : kind === "cur" ? curCellText(r, mixed)
          : kind === "price" ? priceText(r)
          : kind === "chg" ? chgCellText(r)
          : noteText(r, g.closed === true)
        w = Math.max(w, fm.advanceWidth(text))
      }
    }
    return Math.ceil(w) + Style.spaceReal(2)
  }

  readonly property real labelColW: columnWidth("label")
  readonly property real curColW: columnWidth("cur")
  readonly property real priceColW: columnWidth("price")
  readonly property real chgColW: columnWidth("chg")
  readonly property real noteColW: columnWidth("note")

  // Deterministic table geometry: every wrapped column is one 4-column grid of
  // the shared widths, so columns and bands align without measuring the scene —
  // and the card width is DERIVED from these measurements (content never paints
  // outside the panel; the note column is never elided).
  readonly property real colGap: Style.space(24)
  readonly property real rowIndent: Style.space(14)
  readonly property real gridW: labelColW + priceColW + chgColW + noteColW + Style.space(14) * 3
    + (anyMixedCurrency ? curColW + Style.space(14) : 0)

  // Widest group header ("<glyph> Name (source)"), measured bold — headers own
  // a full line above their rows, so a long provider list must also fit.
  FontMetrics {
    id: boldFm
    font.family: root.panelFont
    font.pixelSize: Style.font.title
    font.bold: true
  }

  function headerWidth() {
    var w = 0
    void boldFm.font.pixelSize // re-measure when the font scale changes
    for (var i = 0; i < groups.length; i++) {
      var g = groups[i]
      var t = String(g.glyph || "") + "  " + String(g.label) + "  (" + (g.sources || []).join("·") + ")"
      var cur = groupCurrencyInfo(g)
      if (cur.code !== "") t += "  · " + cur.code.toUpperCase()
      if (g.closed === true) t += "   closed"
      w = Math.max(w, boldFm.advanceWidth(t))
    }
    return Math.ceil(w) + Style.spaceReal(4)
  }

  readonly property real headerW: headerWidth()

  // A wrapped column is as wide as its indented grid or its widest header.
  readonly property real colW: Math.max(gridW + rowIndent, headerW)
  readonly property real tableW: {
    var maxCols = 0
    for (var i = 0; i < bands.length; i++) maxCols = Math.max(maxCols, bands[i].length)
    if (maxCols === 0) return Style.space(200)
    return maxCols * colW + (maxCols - 1) * colGap
  }

  // KeyboardPanel's contentWidth is the FULL card width (the content holder is
  // inset by padding + borders), so the card must add that inset around tableW.
  readonly property real cardWInset: panelSurface.padding * 2
    + Border.left(panelSurface.borderSpec) + Border.right(panelSurface.borderSpec)

  // Most side-by-side columns the screen can hold. When this cap bites, the
  // layout wraps into MORE bands (fewer columns each) instead of overflowing.
  readonly property int maxFitColumns: {
    var avail = panelSurface.availableCardWidth - cardWInset
    if (avail <= 0 || colW <= 0) return 1
    return Math.max(1, Math.floor((avail + colGap) / (colW + colGap)))
  }

  // ---- Auto-fit line budget: how many layout lines one column may hold on
  //      this screen. Pure MEASUREMENT — it is passed to the CLI as
  //      --rows-per-column, where the core packs with it (only when the
  //      config leaves tooltip_rows_per_column at 0). Measured heights:
  //        row line    = body line height + grid rowSpacing
  //        header line = title-bold line height + header->grid gap + the
  //                      inter-segment gap its section adds to the column
  //      The header share of a candidate column is bounded by the worst-case
  //      count of headers inside ANY window of that many consecutive lines
  //      (+1 for a possible continuation header), so tiny adjacent groups
  //      can't overflow the estimate. Iterates to a fixed point (shrinking
  //      the budget can only shrink the window's header count, so it
  //      converges monotonically).
  readonly property real rowUnitH: fm.height + Style.space(4)
  readonly property real headerUnitH: boldFm.height + Style.space(5) + Style.space(16)
  // Footer (separator + gaps + status line) reserved off the top of the budget.
  readonly property real footerReserveH: fm.height + Style.space(24)

  // ---- Summary chip + lifecycle status colors (theme-derived only).
  readonly property var summaryAvg: snapshot ? snapshot.avg_change_pct : null
  readonly property string summaryDir: typeof summaryAvg === "number"
    ? (summaryAvg > 0 ? "up" : (summaryAvg < 0 ? "down" : "flat")) : "flat"
  // The panel's average is ON by default, unlike the bar strip's. The two
  // surfaces have opposite constraints: the strip competes for bar width, so it
  // stays opt-in, while the hero has a slot reserved for exactly this and
  // "how is the watchlist doing overall" is the first thing a reader opening
  // the panel wants. `hide` is an explicit opt-out and still silences both.
  //
  // Read the chip for what it is: an UNWEIGHTED mean of each asset's daily
  // change. tickerbar models no holdings, so it cannot be a portfolio return —
  // BTC and a corporate bond count the same.
  readonly property bool summaryChipVisible: summaryMode !== "hide" && typeof summaryAvg === "number"

  // ---- Hero counts. The open/closed tally is not decoration: it is what
  //      explains why part of the table reads "closed" at a glance, which
  //      nothing else in the panel said.
  readonly property int assetCount: {
    var n = 0
    for (var i = 0; i < groups.length; i++) n += (groups[i].rows || []).length
    return n
  }
  readonly property int openGroupCount: {
    var n = 0
    for (var i = 0; i < groups.length; i++) if (groups[i].closed !== true) n++
    return n
  }
  readonly property string heroMeta: {
    if (assetCount === 0) return ""
    var parts = [assetCount + (assetCount === 1 ? " asset" : " assets")]
    if (groups.length > 0)
      parts.push(openGroupCount + " of " + groups.length
        + (groups.length === 1 ? " market" : " markets") + " open")
    return parts.join(" · ")
  }

  // Height the hero takes off the column budget. The summary chip used to own
  // this reserve on its own; it now rides in the hero's trailing slot, so the
  // hero is close to free whenever the chip is enabled and costs one line when
  // it is not. This panel is the densest of the family — the budget decides how
  // many rows fit per column, so hero chrome is paid for in rows.
  readonly property real heroReserveH: snapshot ? Style.font.display + Style.space(26) : 0

  readonly property color lifecycleColor: lifecycle === "error" || lifecycle === "partial"
    ? panelDirColor("down")
    : (lifecycle === "stale" ? mutedFg : panelDirColor("up", mutedFg, 0.8))

  // Worst-case count of header lines inside any window of `w` consecutive
  // layout lines, given the header line positions.
  function maxHeadersInWindow(headerPos, w) {
    var m = 0
    for (var a = 0; a < headerPos.length; a++) {
      var c = 0
      for (var b = a; b < headerPos.length && headerPos[b] < headerPos[a] + w; b++) c++
      if (c > m) m = c
    }
    return m
  }

  readonly property int autoLineBudget: {
    var headerPos = []
    var totalLines = 0
    for (var i = 0; i < groups.length; i++) {
      headerPos.push(totalLines)
      totalLines += 1 + (groups[i].rows || []).length
    }
    var avail = panelSurface.availableCardHeight - panelSurface.verticalContentInset - footerReserveH - heroReserveH
    if (totalLines === 0) {
      // No content yet (first run): optimistic row-only estimate minus a
      // small header reserve; the post-fetch measurement refines it.
      if (avail <= 0 || rowUnitH <= 0) return 40
      return Math.max(4, Math.floor(avail / rowUnitH) - 8)
    }
    if (avail <= 0 || rowUnitH <= 0) return totalLines

    var budget = Math.max(4, Math.floor(avail / rowUnitH)) // optimistic upper bound
    for (var it = 0; it < 6; it++) {
      var h = Math.min(headerPos.length + 1, maxHeadersInWindow(headerPos, budget) + 1) // +1 continuation
      var rowsFit = Math.floor((avail - h * headerUnitH) / rowUnitH)
      var next = Math.max(4, h + rowsFit)
      if (next >= budget) break
      budget = next
    }
    return budget
  }

  // ---- Packed layout, decided by the CORE (render::pack_columns — the exact
  //      code path that renders the waybar tooltip). The QML makes ZERO packing
  //      decisions: it measures its fit (autoLineBudget / maxFitColumns),
  //      passes the measurements to the CLI as --rows-per-column /
  //      --max-columns, and draws the bands -> columns -> segments the core
  //      returns (headers-never-orphaned, "(cont.)", banding all pre-decided).
  //      This only dereferences the plan into renderable objects.
  readonly property var bands: {
    if (!snapshot || !snapshot.layout || !snapshot.layout.bands) return []
    var out = []
    var plan = snapshot.layout.bands
    for (var b = 0; b < plan.length; b++) {
      var band = []
      for (var c = 0; c < plan[b].length; c++) {
        var col = []
        for (var s = 0; s < plan[b][c].length; s++) {
          var seg = plan[b][c][s]
          var g = groups[seg.group]
          if (!g) continue
          var cur = groupCurrencyInfo(g)
          col.push({
            label: String(g.label),
            glyph: String(g.glyph || ""),
            source: (g.sources || []).join("·"),
            closed: g.closed === true,
            continued: seg.continued === true,
            currency: cur.code,
            mixedCurrency: cur.mixed,
            rows: (g.rows || []).slice(seg.start, seg.start + seg.len)
          })
        }
        band.push(col)
      }
      out.push(band)
    }
    return out
  }

  // Flatten one group's rows into GridLayout cells (4 per row).
  function groupCells(rows, groupClosed, groupMixed) {
    var out = []
    for (var i = 0; i < rows.length; i++) {
      var r = rows[i]
      var dim = r.market === "closed" || r.state === "missing" || r.state === "error"
      out.push({ kind: "label", text: String(r.label), dir: null, dim: dim })
      // The currency micro-column exists only while some group mixes currencies
      // (grid column count must stay uniform across every section).
      if (anyMixedCurrency)
        out.push({ kind: "cur", text: curCellText(r, groupMixed === true), dir: null, dim: true })
      out.push({ kind: "price", text: priceText(r), dir: r.direction, dim: dim })
      out.push({ kind: "chg", text: chgCellText(r), dir: r.direction, dim: dim })
      out.push({ kind: "note", text: noteText(r, groupClosed === true), dir: null, dim: true })
    }
    return out
  }

  // ---- Lifecycle: open/close/summon contract (same shape as weather).
  function open() {
    openedFromHotkey = false
    setCenterHoverRevealSuppressed(false)
    root.controller.show()
    root.refresh()
  }

  function openFromHotkey() {
    openedFromHotkey = true
    root.controller.show()
    root.refresh()
    Qt.callLater(function() {
      if (root.opened) setCenterHoverRevealSuppressed(true)
    })
  }

  function close() {
    setCenterHoverRevealSuppressed(false)
    root.controller.hide()
  }

  function toggle() {
    if (root.opened) root.close()
    else root.openFromHotkey()
  }

  // The shell's base handler covers open/close/show/hide/toggle; this one adds
  // `refresh` so a keybind or a script can force a fetch without opening the
  // panel. Overriding means restating the five, so `manageIpc: false` above
  // turns the base one off and this is the only handler on the target.
  IpcHandler {
    target: root.ipcTarget

    function open(): void { root.openFromHotkey() }
    function close(): void { root.close() }
    function show(): void { root.openFromHotkey() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function refresh(): void { root.refresh() }
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.barIdentity, direction)
    return false
  }

  function setCenterHoverRevealSuppressed(value) {
    if (root.bar && "centerHoverRevealSuppressed" in root.bar)
      root.bar.centerHoverRevealSuppressed = value
  }

  // ---- Data flow: poll the CLI, parse, keep last-known-good on any failure,
  //      and surface run failures explicitly (never a silent forever-loading).
  //      Exit and collector completion are joined before finalizing; a failed
  //      start (collector never fires) is bounded by the fallback timer.
  property bool collectorDone: true
  property bool processDone: true

  // A fetch is in flight. BOTH halves matter: the exit code and the collected
  // stdout arrive in either order, which is exactly why maybeFinalize() waits
  // for the pair. The refresh button gates on this, not on collectorDone alone
  // — otherwise it re-enables in the gap between the two signals and a click
  // there queues a second run through pendingCmd, which is the one thing its
  // disabled state promises cannot happen.
  readonly property bool fetchBusy: !collectorDone || !processDone
  property string capturedText: ""
  property int exitCode: 0
  property var pendingCmd: null

  // True when onExited fired for the current run. A missing command emits
  // no exited. This separates "could not start" from "ran, no output".
  property bool sawExit: false

  // True only when the run could not START. Gates the copy button.
  // Operational errors never set it.
  property bool notInstalled: false

  // One constant, two users: the error message shows it and the copy
  // button copies it.
  readonly property string installCmd: "yay -S tickerbar-bin"

  // The copy button shows a check for a moment.
  property bool installCopied: false
  Timer {
    id: copiedReset
    interval: 1500
    onTriggered: root.installCopied = false
  }

  function refresh() {
    startRun(buildCmd())
  }

  function startRun(cmd) {
    if (statusProc.running) { pendingCmd = cmd; return } // last-command-wins
    collectorDone = false
    processDone = false
    capturedText = ""
    sawExit = false
    exitCode = 0
    statusProc.command = cmd
    statusProc.running = true
  }

  function maybeFinalize() {
    if (!collectorDone || !processDone) return
    exitFallback.stop()
    finalizeRun()
  }


  // Set when the plugin's own run fails, cleared by the next good parse. ORed
  // into the freshness state so a failure here reads like any other staleness.
  property bool pluginStale: false

  function setError(message) {
    runError = message
    // The last good payload stays on screen — deliberate — but it must stop
    // claiming to be current. Without this the footer keeps printing a plain
    // "Updated HH:MM" for data the CLI can no longer refresh.
    pluginStale = true
  }

  function finalizeRun() {
    notInstalled = false
    var text = capturedText.trim()
    if (text === "") {
      // Empty output has three causes. (1) The tripwire already set an
      // error: keep it. (2) No exited = failed start: report not-installed.
      // (3) The process ran and printed nothing: an operational error,
      // never "not installed".
      if (root.runError !== "") {
        // Already explained (tripwire).
      } else if (!sawExit) {
        notInstalled = true
        setError(binName + " could not start — not installed or not on PATH?\n\n"
                 + "Install it with:  " + installCmd + "\n"
                 + "Then open this panel again.")
      } else {
        setError(binName + " produced no output (exit " + exitCode + ")")
      }
    } else {
      handle(text)
    }
    if (pendingCmd) {
      var c = pendingCmd
      pendingCmd = null
      Qt.callLater(function() { root.startRun(c) })
    }
  }

  function handle(out) {
    // The CLI always exits 0 with a valid schema_version:1 document (errors ride
    // inside it as `error: {message}`), so anything else — waybar-shaped output,
    // truncated JSON — is an explicit error, while the last snapshot stays shown.
    try {
      var d = JSON.parse(out)
      if (!d || d.schema_version !== 1) {
        setError(binName + " returned an unexpected document (not schema_version 1)")
        return
      }
      // A structured failure with nothing to draw (config unreadable, internal error)
      // is the same situation as unparseable output: surface the message, but keep the
      // last good document on screen instead of blanking the table. A document that
      // still carries groups is a real (possibly partial) run and replaces it.
      var hasContent = d.groups && d.groups.length > 0
      if (d.error && d.error.message && !hasContent) {
        setError(String(d.error.message))
        return
      }
      root.snapshot = d
      root.runError = ""
      root.pluginStale = false
      // Fresh content can move the measured fit (row widths, group map). If it
      // differs from what this run was packed with, repack once — the second
      // run is cache-hot, and measurements are stable for identical content,
      // so this cannot loop.
      if (root.autoLineBudget !== root.lastSentBudget || root.maxFitColumns !== root.lastSentMaxCols)
        Qt.callLater(root.refresh)
    } catch (e) {
      setError(binName + " returned unparseable output"
        + (root.exitCode !== 0 ? " (exit " + root.exitCode + ")" : ""))
    }
  }

  Process {
    id: statusProc
    // A command that does not exist gives NEITHER `started` NOR `exited` —
    // Quickshell just drops `running` back to false. That is the only signal a
    // failed start emits, and without this handler the panel sits on its
    // loading text for ever: maybeFinalize() waits on processDone, which
    // nothing would ever set. This IS the first run of anyone who installed
    // the plugin from the marketplace and does not have the CLI yet.
    onRunningChanged: {
      if (running) return
      root.processDone = true
      exitFallback.restart()
      root.maybeFinalize()
    }
    onExited: function(code) {
      root.sawExit = true
      root.exitCode = code
      root.processDone = true
      exitFallback.restart() // failed-start case: the collector may never fire
      root.maybeFinalize()
    }
    stdout: StdioCollector {
      waitForEnd: true
      // A tripwire, not a limit, and it counts UTF-16 units rather than bytes —
      // QML's String.length has no byte view, so a megabyte of units is up to
      // three megabytes of UTF-8. StdioCollector has already buffered the whole
      // stream by the time this runs, so this cannot cap the peak memory — the
      // real bound is in the CLI, which reads every file and every response
      // under a byte cap of its own. What this does is refuse to RETAIN an
      // answer that could not have come from a healthy run, and say so, instead
      // of parsing megabytes of unknown text into the long-lived shell process.
      readonly property int maxChars: 1024 * 1024
      onStreamFinished: {
        if (text.length > maxChars) {
          root.capturedText = ""
          root.setError(root.binName + " returned more than " + (maxChars / 1024) + "K characters — refusing it")
        } else {
          root.capturedText = text
        }
        root.collectorDone = true
        root.maybeFinalize()
      }
    }
  }

  Timer {
    id: exitFallback
    interval: 300
    repeat: false
    onTriggered: {
      root.collectorDone = true // give up on the collector
      root.maybeFinalize()
    }
  }

  Timer {
    interval: root.refreshSeconds * 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  KeyboardPanel {
    id: panelSurface
    anchorItem: root.anchorItem
    owner: root.barIdentity
    bar: root.bar
    open: root.opened
    centerOnBar: true
    focusTarget: keyCatcher
    // Card width derived from the measured table (plus the card's own padding
    // and borders) so the panel always auto-fits its content; the screen-fit
    // clamp should never bite because maxFitColumns already wraps columns
    // into extra bands before that point.
    contentWidth: panelSurface.fittedContentWidth(root.tableW + root.cardWInset)
    contentHeight: panelSurface.fittedContentHeight(tickerColumn.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      // No scrolling: like the waybar tooltip, long watchlists wrap into
      // side-by-side columns (and stacked bands), never a scrollbar.
      Column {
        id: tickerColumn
        spacing: Style.space(14)

        Text {
          textFormat: Text.PlainText
          visible: root.snapshot === null && root.topError === ""
          text: "Fetching prices…"
          color: root.mutedFg
          font.family: root.panelFont
          font.pixelSize: Style.font.bodySmall
          font.italic: true
        }

        // Error card (codexbar's pattern): quiet urgent-tinted surface instead
        // of a bare red line — readable without shouting.
        Rectangle {
          visible: root.topError !== ""
          width: root.tableW
          implicitHeight: errorText.implicitHeight + Style.space(14)
          radius: Style.cornerRadius
          color: Util.alpha(root.panelUrgent, 0.10)
          border.width: 1
          border.color: Util.alpha(root.panelUrgent, 0.30)

          Text {
            id: errorText
            anchors.left: parent.left
            anchors.right: copyInstallButton.visible ? copyInstallButton.left : parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: Style.space(10)
            anchors.rightMargin: Style.space(10)
            text: root.topError
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: root.mutedFg
            font.family: root.panelFont
            font.pixelSize: Style.font.bodySmall
          }

          // Copies installCmd as one argv element: no shell line, no
          // trailing newline. Gated on notInstalled, never on error text —
          // topError also carries CLI errors, and those get no button.
          PanelActionButton {
            id: copyInstallButton
            visible: root.notInstalled
            anchors.right: parent.right
            anchors.rightMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            iconText: root.installCopied ? "󰄬" : "󰆏"
            tooltipText: root.installCopied ? "Copied" : "Copy install command"
            foreground: root.mutedFg
            hoverColor: root.panelFg
            fontFamily: root.panelFont
            fontSize: Style.font.caption
            size: Style.space(20)
            onClicked: {
              Util.execArgv(["wl-copy", root.installCmd])
              root.installCopied = true
              copiedReset.restart()
            }
          }
        }

        // ---- Hero: the panel's identity line. Every sibling widget opens by
        //      naming itself; this one used to drop straight into a 71-row
        //      table with nothing saying what it was. The glyph is the class
        //      mark the stock rows already carry, in the theme accent —
        //      identity, never a gauge (see claudebar/codexbar/logibar).
        //
        //      The watchlist average rides in the trailing slot rather than
        //      becoming the hero's headline number. An equal-weighted mean
        //      across crypto, ARS bonds and treasuries is a mood, not a
        //      portfolio return, and printing it large would promise a
        //      precision it does not have.
        PanelHero {
          visible: root.snapshot !== null
          width: root.tableW
          title: "Watchlist"
          meta: root.heroMeta
          foreground: root.panelFg
          fontFamily: root.panelFont

          iconComponent: Component {
            Text {
              text: "" // nf-fa-line_chart
              textFormat: Text.PlainText
              color: root.panelAccent
              font.family: root.panelFont
              font.pixelSize: Style.font.display
            }
          }

          trailingControl: Component {
            Rectangle {
              visible: root.summaryChipVisible
              radius: height / 2
              implicitWidth: chipRow.implicitWidth + Style.space(20)
              implicitHeight: chipRow.implicitHeight + Style.space(9)
              color: Util.alpha(root.panelDirColor(root.summaryDir), 0.12)
              border.width: 1
              border.color: Util.alpha(root.panelDirColor(root.summaryDir), 0.32)

              Behavior on color { ColorAnimation { duration: 160 } }
              Behavior on border.color { ColorAnimation { duration: 160 } }

              Row {
                id: chipRow
                anchors.centerIn: parent
                spacing: Style.space(7)

                Text {
                  textFormat: Text.PlainText
                  id: chipValue
                  text: root.changeText(root.summaryAvg, root.summaryDir)
                  color: root.panelDirColor(root.summaryDir)
                  font.family: root.panelFont
                  font.pixelSize: Style.font.body
                  font.bold: true
                  renderType: Text.NativeRendering

                  Behavior on color { ColorAnimation { duration: 160 } }
                }

                Text {
                  textFormat: Text.PlainText
                  anchors.baseline: chipValue.baseline
                  text: "avg"
                  color: root.mutedFg
                  font.family: root.panelFont
                  font.pixelSize: Style.font.caption
                }
              }
            }
          }
        }

        PanelSeparator {
          visible: root.snapshot !== null
          width: root.tableW
          foreground: root.panelFg
        }

        // ---- Bands stacked vertically; inside a band, columns sit side by
        //      side; a column is a run of sections (header + aligned 4-column
        //      grid: label | price | change % | note). Every grid shares the
        //      global column widths, so the whole panel reads as one table.
        Repeater {
          model: root.bands

          Column {
            id: bandBlock
            required property var modelData
            required property int index
            spacing: Style.space(14)

            PanelSeparator {
              visible: bandBlock.index > 0
              width: root.tableW
              foreground: root.panelFg
            }

            Row {
              spacing: root.colGap

              Repeater {
                model: bandBlock.modelData

                Item {
                  id: colBlock
                  required property var modelData
                  required property int index
                  width: root.colW
                  implicitHeight: colInner.implicitHeight

                  // Hairline between side-by-side columns — the QML analogue of
                  // the waybar tooltip's │ divider — centered in the gap and
                  // spanning the whole band's height.
                  Rectangle {
                    visible: colBlock.index > 0
                    x: -root.colGap / 2
                    width: 1
                    height: colBlock.parent ? colBlock.parent.height : colBlock.height
                    color: Util.alpha(root.panelFg, 0.10)
                  }

                  Column {
                    id: colInner
                    width: parent.width
                    spacing: Style.space(16)

                    Repeater {
                      model: colBlock.modelData

                      Column {
                        id: segBlock
                        required property var modelData
                        spacing: Style.space(5)

                        // Header line, same hierarchy as the waybar tooltip:
                        // accent class glyph + bold name at row size, muted
                        // "(source)" suffix, muted markers after it. Never
                        // smaller than the rows beneath.
                        Row {
                          spacing: Style.space(7)

                          Text {
                            text: String(segBlock.modelData.glyph || "")
                            textFormat: Text.PlainText
                            visible: text !== ""
                            color: root.panelAccent
                            anchors.baseline: sectionHeader.baseline
                            font.family: root.panelFont
                            font.pixelSize: Style.font.title
                            renderType: Text.NativeRendering
                          }

                          Text {
                            id: sectionHeader
                            // One step above the rows (title vs body): bold alone
                            // does not read as a header in a monospace face.
                            text: String(segBlock.modelData.label)
                            textFormat: Text.PlainText
                            color: root.panelFg
                            font.family: root.panelFont
                            font.pixelSize: Style.font.title
                            font.bold: true
                            renderType: Text.NativeRendering
                          }

                          Text {
                            visible: text !== "()"
                            text: "(" + String(segBlock.modelData.source || "") + ")"
                            textFormat: Text.PlainText
                            color: root.mutedFg
                            anchors.baseline: sectionHeader.baseline
                            font.family: root.panelFont
                            font.pixelSize: Style.font.body
                            renderType: Text.NativeRendering
                          }

                          // Section-wide currency: shown once here when every
                          // priced row in the group shares it (mixed groups get
                          // per-row codes in the grid instead).
                          Text {
                            visible: String(segBlock.modelData.currency || "") !== ""
                            text: "· " + String(segBlock.modelData.currency || "").toUpperCase()
                            textFormat: Text.PlainText
                            color: root.mutedFg
                            anchors.baseline: sectionHeader.baseline
                            font.family: root.panelFont
                            font.pixelSize: Style.font.body
                            renderType: Text.NativeRendering
                          }

                          Text {
                            textFormat: Text.PlainText
                            visible: segBlock.modelData.continued === true
                            anchors.baseline: sectionHeader.baseline
                            text: "(cont.)"
                            color: root.mutedFg
                            font.family: root.panelFont
                            font.pixelSize: Style.font.bodySmall
                          }

                          Text {
                            textFormat: Text.PlainText
                            visible: segBlock.modelData.closed === true
                            anchors.baseline: sectionHeader.baseline
                            text: "\uf04c closed"
                            color: root.mutedFg
                            font.family: root.panelFont
                            font.pixelSize: Style.font.bodySmall
                          }
                        }

                        GridLayout {
                          x: root.rowIndent
                          columns: root.anyMixedCurrency ? 5 : 4
                          rowSpacing: Style.space(4)
                          columnSpacing: Style.space(14)

                          Repeater {
                            model: root.groupCells(segBlock.modelData.rows || [], segBlock.modelData.closed, segBlock.modelData.mixedCurrency)

                            Text {
                              required property var modelData
                              text: modelData.text
                              textFormat: Text.PlainText
                              color: modelData.dim ? root.mutedFg
                                : (modelData.kind === "label" ? root.panelFg : root.panelDirColor(modelData.dir))
                              font.family: root.panelFont
                              font.pixelSize: Style.font.body
                              renderType: Text.NativeRendering
                              horizontalAlignment: modelData.kind === "price" || modelData.kind === "chg"
                                ? Text.AlignRight : Text.AlignLeft
                              Layout.preferredWidth: modelData.kind === "label" ? root.labelColW
                                : modelData.kind === "cur" ? root.curColW
                                : modelData.kind === "price" ? root.priceColW
                                : modelData.kind === "chg" ? root.chgColW : root.noteColW
                              Layout.alignment: Qt.AlignVCenter
                                | (modelData.kind === "price" || modelData.kind === "chg" ? Qt.AlignRight : Qt.AlignLeft)
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }

        // ---- Freshness footer: when the data is from, plus an inline
        //      refresh. The button re-runs the CLI right now — the same
        //      forced refresh the bar's middle-click does — so a stale panel
        //      can be corrected without closing it, and it is disabled while
        //      a fetch is already in flight so clicks cannot queue up. The
        //      rule and the row are always shown: the button has to stay
        //      reachable exactly when there is no timestamp to print yet.
        //
        //      The lifecycle suffix ("· stale", "· partial data") keeps its own
        //      tint so an unhealthy fetch still reads at a glance; the colored
        //      dot it replaces said the same thing without naming it.
        Column {
          // Same gap the card puts BELOW this line (popupPadding, 14), so the
          // footer sits centered between the rule and the card's bottom edge
          // instead of hugging the rule. It is also the outer column's own
          // rhythm, which is what the sibling widgets get for free by keeping
          // rule and footer as siblings in their main column.
          spacing: Style.space(14)

          PanelSeparator { width: root.tableW; foreground: root.panelFg }
          Item {
            width: root.tableW
            implicitHeight: Math.max(footerRow.implicitHeight, refreshButton.implicitHeight)

            Row {
              id: footerRow
              anchors.left: parent.left
              anchors.verticalCenter: parent.verticalCenter
              spacing: Style.space(4)

              Text {
                textFormat: Text.PlainText
                anchors.verticalCenter: parent.verticalCenter
                text: "󰅐  Updated " + (root.updatedText !== "" ? root.updatedText : "—")
                color: root.mutedFg
                font.family: root.panelFont
                font.pixelSize: Style.font.caption
              }

              Text {
                visible: root.lifecycle !== "" && root.lifecycle !== "ok"
                anchors.verticalCenter: parent.verticalCenter
                text: " · " + (root.lifecycle === "partial" ? "partial data" : root.lifecycle)
                textFormat: Text.PlainText
                // The family's freshness tint, NOT the market's down color:
                // "these prices are old" and "these prices fell" are unrelated
                // facts, and painting them the same red conflates them. Every
                // non-fresh lifecycle takes the same tone.
                color: root.freshnessWarn
                font.family: root.panelFont
                font.pixelSize: Style.font.caption
              }
            }

            PanelActionButton {
              id: refreshButton
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              // nf-md-refresh (U+F0450). Written literally: a JS "\\u" escape takes
              // exactly FOUR hex digits, so "\\uf0450" is U+F045 followed by a "0".
              iconText: "󰑐"
              tooltipText: "Refresh now"
              foreground: root.mutedFg
              hoverColor: root.panelFg
              fontFamily: root.panelFont
              fontSize: Style.font.caption
              size: Style.space(20)
              enabled: !root.fetchBusy
              onClicked: root.refresh()
            }
          }
        }
      }
    }
  }
}
