pragma ComponentBehavior: Bound
import QtQuick
import Quickshell
import qs.Commons
import qs.Ui

// Compact bar strip for tickerbar: the CLI-curated subset (config `display.bar`,
// order preserved) as "LABEL price ±chg%" segments, direction subtly tinted with
// theme-derived colors. Click opens the market table panel (TickerPanel.qml,
// which owns the data); middle click refreshes.
BarWidget {
  id: root
  moduleName: "mryll.tickerbar"

  // Typed view of the loaded panel (TickerPanel is the sibling TickerPanel.qml;
  // the distinct name avoids any clash with qs.Ui's Panel base type).
  readonly property TickerPanel marketPanel: panelLoader.item as TickerPanel

  function injectPanel() {
    var target = root.marketPanel
    if (!target) return
    target.bar = root.bar
    target.settings = root.settings
    target.anchorItem = button
    target.hostWidget = root
  }

  function refresh() {
    if (root.marketPanel) root.marketPanel.refresh()
  }

  function togglePanel() {
    if (root.marketPanel) root.marketPanel.toggle()
  }

  // Shape contract for shell.summon/hide/toggle routing (Bar.findPanelWidget
  // requires open/close/opened on the bar-widget root) — same as weather.
  readonly property bool opened: marketPanel ? marketPanel.opened === true : false

  function open() {
    if (root.marketPanel) root.marketPanel.openFromHotkey()
  }

  function close() {
    if (root.marketPanel) root.marketPanel.close()
  }

  readonly property bool popoutSwitchClosing: marketPanel ? marketPanel.popoutSwitchClosing === true : false

  function closeForPopoutSwitch() {
    if (root.marketPanel) root.marketPanel.closeForPopoutSwitch()
  }

  readonly property var entries: marketPanel ? marketPanel.barEntries : []
  readonly property bool hasData: marketPanel ? marketPanel.snapshot !== null : false
  readonly property string lifecycle: marketPanel ? marketPanel.lifecycle : ""
  readonly property bool errored: marketPanel ? marketPanel.topError !== "" : false
  // Serving cached prices: a failed run, or a snapshot the core marked stale.
  readonly property bool stripStale: marketPanel
    ? (marketPanel.lifecycle === "stale" || marketPanel.pluginStale === true) : false

  // barForeground (not foreground) so the strip follows the bar's transparent-mode color.
  readonly property color baseFg: root.bar ? root.bar.barForeground : Color.foreground
  // Same class glyph the waybar tooltip uses for stocks (nf-fa-line_chart);
  // shown alone while loading, on error, and on vertical bars.
  readonly property string glyph: ""
  // ---- Width-aware degradation, PER BAR INSTANCE (one bar per monitor; the
  //      2560px bar can keep the full strip while the 1080px one trims).
  //      The strip must never paint over neighboring sections: from this
  //      window's real geometry, compute the width the bar can actually give
  //      this widget, then drop entries from the END (config `display.bar`
  //      order is priority order) down to the glyph as the floor.
  //
  //      Where the geometry comes from. Since omarchy 1702cf0 a third-party
  //      widget receives a PluginBarApi facade as `bar` (shell/Ui/
  //      PluginBarApi.qml), not the Bar: it has no moduleSlots, slotWindow,
  //      centerAnchor or layoutEntries, and its `moduleWidgets(id)` answers
  //      only for this widget's own id (Bar.pluginBarApiFor wires it that
  //      way). An earlier version read those Bar internals through `bar`, so
  //      its guard failed on every evaluation and the strip never
  //      self-limited (PR #2's diagnosis). The neighbors are still visible,
  //      though: this widget is a visual child of the bar window's item
  //      tree, and the bar puts every module in a slot Item that declares
  //      `region` ("left"|"center"|"right") and `moduleName`. So walk THIS
  //      window's tree (the bar builds one tree per monitor, which scopes
  //      multi-monitor for free), collect those slots and model the layout
  //      from their widths and positions — the same model Bar.qml lays out:
  //      left/right sections at the edges, the center block either centered
  //      as one row or split in two flanks around a pinned centerAnchor.
  //
  //      Loop safety: nothing here reads this widget's own slot width, and
  //      the positions it reads either do not follow our width (the anchor,
  //      the outer sections) or only decide a side that cannot change.
  //      When the tree does not look like that, fall back to a fraction of
  //      the window (`stripMaxPercent`), so the failure mode is "capped a bit
  //      blindly", never "painted over the clock".
  readonly property real stripMaxFraction: {
    var v = Number(root.settings ? root.settings.stripMaxPercent : undefined)
    return v >= 10 && v <= 90 ? v / 100 : 0.30
  }

  // The bar's slot contract: the ancestor that hosts this widget, and the
  // items the tree walk counts as neighbors.
  function isModuleSlot(item) {
    return !!item && typeof item.region === "string" && typeof item.moduleName === "string"
  }

  function hostSlot() {
    var n = root.parent
    for (var depth = 0; n && depth < 6; depth++) {
      if (isModuleSlot(n)) return n
      n = n.parent
    }
    return null
  }

  // Every module slot under `item`. Slots do not nest, so the walk stops at
  // each one (a widget's own panel or loader children are never visited).
  // Reading `children` on the way down makes the binding follow rebuilds.
  function collectSlots(item, out, budget) {
    if (!item || budget.n-- <= 0) return
    if (isModuleSlot(item)) { out.push(item); return }
    var kids = item.children
    for (var i = 0; i < kids.length; i++) collectSlots(kids[i], out, budget)
  }

  // x of `item`'s left edge in `top`'s coordinates, summed by hand rather
  // than mapToItem so each `x` on the way is a binding dependency.
  function leftIn(item, top) {
    var x = 0
    for (var n = item; n && n !== top; n = n.parent) x += n.x
    return x
  }

  function sumWidths(slots) {
    var total = 0
    for (var i = 0; i < slots.length; i++) total += slots[i].width
    return total
  }

  readonly property real stripMaxWidth: {
    if (!root.bar || root.vertical) return -1
    var win = root.QsWindow.window
    if (!win || !(win.width > 0)) return -1
    var W = win.width
    var margin = Style.space(8)   // the bar's left/right section edge margins
    var safety = Style.space(12)  // breathing gap kept before a neighbor section
    var blind = Math.max(0, W * root.stripMaxFraction - margin - safety)

    var mine = hostSlot()
    if (!mine) return blind
    var top = mine
    while (top.parent) top = top.parent
    var all = []
    collectSlots(top, all, { n: 4000 })

    // Structure is read from every slot, hidden ones included (a configured
    // anchor whose widget is hidden still splits the center in flanks pinned
    // to the middle); only the measured sums skip what is not on screen. The
    // bar always builds the anchor slot, with an empty moduleName when no
    // anchor is configured: that one is not structure.
    var left = [], center = [], right = []
    var oneRow = true
    for (var i = 0; i < all.length; i++) {
      var s = all[i]
      if (s === mine || s.moduleName === "") continue
      if (s.region === "center" && s.parent !== mine.parent) oneRow = false
      if (!s.visible || !(s.width > 0)) continue
      if (s.region === "left") left.push(s)
      else if (s.region === "right") right.push(s)
      else if (s.region === "center") center.push(s)
    }

    // Inner edges of the outer sections, measured; the bar margin when empty.
    var half = W / 2
    var leftBound = margin
    var rightBound = W - margin
    var k
    if (mine.region !== "left")
      for (k = 0; k < left.length; k++) leftBound = Math.max(leftBound, leftIn(left[k], top) + left[k].width)
    if (mine.region !== "right")
      for (k = 0; k < right.length; k++) rightBound = Math.min(rightBound, leftIn(right[k], top))

    if (mine.region === "left") {
      // Grows rightward from the bar edge, up to the center block (or the
      // right section when there is no center block).
      var bound = rightBound
      for (k = 0; k < center.length; k++) bound = Math.min(bound, leftIn(center[k], top))
      return Math.max(0, bound - margin - sumWidths(left) - safety)
    }
    if (mine.region === "right") {
      var edge = leftBound
      for (k = 0; k < center.length; k++) edge = Math.max(edge, leftIn(center[k], top) + center[k].width)
      return Math.max(0, W - margin - sumWidths(right) - edge - safety)
    }
    if (mine.region !== "center") return blind

    if (oneRow) {
      // One block centered on the bar (the plain center row, or this widget
      // alone): symmetric about the middle, bounded by the nearer section.
      return Math.max(0, 2 * Math.min(half - leftBound, rightBound - half) - sumWidths(center) - safety)
    }

    // Anchored center: the pinned module straddles the middle and the flanks
    // grow outward from its edges. Which flank we are on is read from our
    // position against the anchor's middle; that side never changes with
    // our width.
    var anchor = null
    for (k = 0; k < center.length; k++) {
      var l = leftIn(center[k], top)
      if (l <= half && l + center[k].width >= half) anchor = center[k]
    }
    var aLeft = anchor ? leftIn(anchor, top) : half
    var aRight = anchor ? aLeft + anchor.width : half
    var aMid = (aLeft + aRight) / 2
    var before = 0
    var after = 0
    for (k = 0; k < center.length; k++) {
      if (center[k] === anchor) continue
      var mid = leftIn(center[k], top) + center[k].width / 2
      if (mid >= aMid) after += center[k].width
      else before += center[k].width
    }
    if (!anchor) {
      // Nothing else straddles the middle: this widget IS the anchor (it is
      // centered, so it could take twice the tighter side) or the anchor is
      // hidden and we are a flank starting at the middle. The tighter side,
      // once, is under both.
      return Math.max(0, Math.min(half - leftBound - before, rightBound - half - after) - safety)
    }
    return leftIn(mine, top) >= aMid
      ? Math.max(0, rightBound - aRight - after - safety)
      : Math.max(0, aLeft - leftBound - before - safety)
  }

  // Measure entries with the same font the strip renders in.
  FontMetrics {
    id: barFm
    font.family: root.bar ? root.bar.fontFamily : Style.font.family
    font.pixelSize: Style.font.body
  }

  // Degradation ladder: how many leading entries fit the available width
  // (with the truncation marker when trimmed). 0 = glyph-only floor.
  readonly property int fitCount: {
    var n = entries.length
    if (n === 0 || root.vertical) return 0
    var max = stripMaxWidth
    if (max < 0) return n // no geometry yet: render fully, settles next frame
    void barFm.font.pixelSize
    var sepW = barFm.advanceWidth("  ·  ")
    var markerW = sepW + barFm.advanceWidth("…")
    var pad = Style.spaceReal(8.5) * 2 // WidgetButton's horizontal margins
    // The stale mark is painted after the entries, so it has to be budgeted
    // here too — the same omission that made the class glyph overflow.
    var staleW = root.stripStale ? barFm.advanceWidth("  ") : 0
    var used = pad + staleW
    var fit = 0
    for (var i = 0; i < n; i++) {
      // Measure exactly what `pieces` paints, class mark included. Leaving the
      // glyph out made every glyph-bearing entry wider than the width used to
      // decide whether it fit, so near a boundary the strip kept one entry too
      // many and could paint over the neighbouring module.
      var e = entries[i]
      var lead = (e.glyph !== undefined && e.glyph !== "") ? e.glyph + " " : ""
      var w = barFm.advanceWidth(lead + e.label + " " + e.value)
      var need = used + (i > 0 ? sepW : 0) + w
      // Non-final entries must also leave room for the "…" marker.
      if (need + (i < n - 1 ? markerW : 0) > max) break
      used = need
      fit = i + 1
    }
    return fit
  }

  readonly property bool truncated: fitCount < entries.length
  readonly property bool glyphOnly: root.vertical || entries.length === 0 || fitCount === 0
  // Below the glyph floor: when the flank has no room even for the glyph,
  // vanish instead of adding to the pile (WidgetButton hides on empty text).
  readonly property bool hiddenByWidth: !root.vertical && stripMaxWidth >= 0
    && stripMaxWidth < barFm.advanceWidth(glyph) + Style.spaceReal(8.5) * 2

  // Monochrome strip ("none" / "panel-only" in the plugin's `colorMode`). The
  // muted closed-market dimming is not color, so it stays in every mode.
  readonly property bool barColored: marketPanel ? marketPanel.barColored === true : false

  function entryColor(entry, base) {
    if (!root.marketPanel || !root.barColored) return base
    // No strength: land on the core's published direction colour, the same one the
    // Waybar bar text paints, so the two bars never disagree about a number.
    return root.marketPanel.dirColorFor(entry.dir, base)
  }

  // Flat list of colored text runs: "LABEL " in the plain foreground, the
  // numbers tinted by direction; closed-market entries dimmed whole. Entries
  // are separated by a faint middle dot instead of bare whitespace. Only the
  // first `fitCount` entries render; a muted ellipsis marks a trimmed strip.
  readonly property var pieces: {
    var out = []
    var shown = Math.min(fitCount, entries.length)
    for (var i = 0; i < shown; i++) {
      var e = entries[i]
      var entryBase = e.muted ? Qt.darker(baseFg, 1.55) : baseFg
      if (i > 0) out.push({ text: "  ·  ", color: Util.alpha(baseFg, 0.35) })
      // Class mark, in the same tone as the label: it says WHICH section the
      // asset belongs to, not how it is doing, so it must not take the
      // direction color the numbers carry. Absent for the summary entry.
      if (e.glyph !== undefined && e.glyph !== "")
        out.push({ text: e.glyph + " ", color: entryBase })
      out.push({ text: e.label + " ", color: entryBase })
      out.push({ text: e.value, color: e.muted ? entryBase : entryColor(e, entryBase) })
    }
    if (shown > 0 && truncated)
      out.push({ text: "  ·  …", color: Util.alpha(baseFg, 0.35) })
    // The family's stale mark (nf-fa-pause): the prices keep their direction
    // colors and staleness gets its own glyph, never a tint over the numbers.
    if (shown > 0 && root.stripStale)
      out.push({ text: "  ", color: Qt.darker(baseFg, 1.55) })
    return out
  }

  // Plain concatenation of the runs — sizes the WidgetButton (its own label is
  // hidden under the colored Row) and is what a vertical bar falls back from.
  // Empty when there is no width even for the glyph (widget disappears).
  readonly property string plainText: {
    if (hiddenByWidth) return ""
    if (glyphOnly) return glyph
    var s = ""
    for (var i = 0; i < pieces.length; i++) s += pieces[i].text
    return s
  }

  // Glyph tint when the strip is collapsed: the shared direction of all
  // entries, or plain foreground when mixed/empty.
  readonly property color glyphColor: {
    if (entries.length === 0 || !marketPanel || !barColored) return baseFg
    var dir = entries[0].dir
    for (var i = 1; i < entries.length; i++) {
      if (entries[i].dir !== dir) return baseFg
    }
    return marketPanel.dirColorFor(dir, baseFg)
  }

  // How wide the bar's open-panel underline should be. Without this hint the bar
  // falls back to 55% of the SLOT, which reads as a dot under a narrow widget
  // but as a bar that visibly stops short under a wide one. The painted content
  // is the honest extent, so the mark tracks what the widget draws instead of a
  // fraction of the box it happens to sit in. (Same hint the first-party clock
  // gives; it passes its label width.)
  // Extent of the open-panel mark, and the width the content row is centered
  // against. The bar computes the mark as
  //     width = Math.round(hint);  x = Math.round((slot.width - width) / 2)
  // so the row must be centered with the SAME rounded width and the SAME
  // formula. Letting `anchors.centerIn` center the row against its own
  // fractional implicitWidth instead puts the two on different pixels whenever
  // the slot width is fractional (it usually is: font metrics are not integers),
  // and the mark reads as shifted under the text.
  readonly property real markExtent: Math.round(contentRow.implicitWidth)
  readonly property real openPanelIndicatorWidth: root.glyphOnly
    ? button.labelWidth : markExtent

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onBarChanged: injectPanel()
  onSettingsChanged: injectPanel()

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("TickerPanel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.plainText
    labelVisible: root.glyphOnly
    // The (config-derived) strip is drawn by the PlainText runs below; when they
    // are shown, size from them instead of the hidden AutoText label.
    fixedWidth: root.glyphOnly || root.vertical ? -1 : contentRow.implicitWidth + button.scaledHorizontalMargin * 2
    foreground: root.glyphOnly ? root.glyphColor : root.baseFg
    // Dim on error but keep the last-known-good strip rendered; the message is
    // readable in the panel.
    // Dim ONLY when there is no snapshot to show. A failed fetch behind cached
    // prices used to drop the whole strip to 45% opacity, which restated the
    // failure in the same channel the direction colors already use. The prices
    // on screen are still the last true ones; the panel footer names the
    // lifecycle ("· stale", "· partial data") and the waybar module gets the
    // same thing as a CSS class.
    dimmed: !root.hasData
    // Tooltip suppressed: the panel is the detail view.
    tooltipText: ""

    onPressed: function(b) {
      if (b === Qt.MiddleButton) root.refresh()
      else root.togglePanel()
    }

    Row {
      id: contentRow
      visible: !root.glyphOnly
      x: Math.round((parent.width - root.markExtent) / 2)
      anchors.verticalCenter: parent.verticalCenter
      spacing: 0

      Repeater {
        model: root.pieces

        Text {
          required property var modelData
          text: modelData.text
          textFormat: Text.PlainText
          color: modelData.color
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.body
          renderType: Text.NativeRendering
          verticalAlignment: Text.AlignVCenter

          Behavior on color {
            enabled: !root.bar || root.bar.foregroundAnimationEnabled
            ColorAnimation { duration: 160 }
          }
        }
      }
    }
  }
}
