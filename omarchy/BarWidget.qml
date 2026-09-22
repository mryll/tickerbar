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
  //      An earlier version of this sized itself against `bar.moduleSlots` /
  //      `bar.slotWindow` / `bar.sameWindow` / `bar.centerAnchor` /
  //      `bar.layoutEntries` / `bar.entryId` to precisely bound itself
  //      against its actual neighbors. None of that exists on the object a
  //      third-party widget is actually handed (`root.bar` is a
  //      `PluginBarApi` instance — see Ui/PluginBarApi.qml — which exposes
  //      only scalar bar state and a few scoped callbacks, deliberately not
  //      host-Bar internals). So that geometry read silently failed its own
  //      guard on every evaluation and fell through to "no geometry yet:
  //      render fully" — i.e. the strip never self-limited at all, which is
  //      how it ended up painted straight over the clock. There is no way
  //      for a plugin to see sibling widget widths in this API, so the best
  //      available fix is a conservative fixed fraction of the window width.
  readonly property real stripMaxWidth: {
    if (!root.bar || root.vertical) return -1
    var win = root.QsWindow.window
    if (!win || !(win.width > 0)) return -1
    var W = win.width
    var margin = Style.space(8)   // the bar's left/right section edge margins
    var safety = Style.space(12)  // breathing gap kept before a neighbor section

    // PluginBarApi (the facade a third-party widget actually receives — see
    // Ui/PluginBarApi.qml) exposes NONE of moduleSlots/slotWindow/sameWindow/
    // layoutEntries/entryIndex/entryId/centerAnchor: those are host-internal
    // Bar.qml fields, deliberately not part of the plugin-facing surface
    // ("the facade avoids direct host-Bar injection"). So there is no way
    // for this widget to see sibling widths or where the center block sits.
    // The only real signal available is the window width itself. Cap to a
    // conservative fraction of it so the strip always self-limits instead
    // of rendering every entry at full width straight through the clock —
    // which is what an unconditional "no geometry yet" bailout used to do
    // on every single evaluation (the previous version of this code assumed
    // bar.slotWindow/sameWindow existed and silently short-circuited here).
    var conservativeFraction = 0.30
    var available = W * conservativeFraction - margin - safety
    return Math.max(0, available)
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
