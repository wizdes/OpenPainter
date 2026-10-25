// Effect dialogs (M24): one sheet built from an effect's schema, with live
// preview through the engine preview session.
//
// - Every change updates the preview. At most one render is in flight; changes
//   that arrive meanwhile are coalesced and the newest values are sent when the
//   render ends.
// - A render that takes longer than 200 ms shows a progress bar.
// - OK commits one history step (the engine re-runs the effect with the final
//   values, so the result equals one plain run). Cancel ends the session; the
//   document and history stay as they were.
// - Effects > Repeat (Cmd+F, paint.net's Ctrl+F) runs the last effect again with
//   the same values, seed included, without a dialog. paint.net 5.1 documents
//   no "repeat with dialog" shortcut: its Effects menu page
//   (https://www.getpaint.net/doc/latest/EffectsMenu.html) lists only Repeat
//   (Ctrl+F), and its keyboard page
//   (https://www.getpaint.net/doc/latest/KeyboardMouseCommands.html) gives
//   Ctrl+Shift+F to Image > Flatten, which is Shift-Cmd-F here. So there is no
//   such item.
import AppKit

@MainActor
public final class EffectDialogController: NSWindowController {
    public let editor: Editor
    public let schema: EffectSchema
    public private(set) var values: [String: ParamValue]
    /// True once preview.begin was sent.
    public private(set) var sessionStarted = false
    /// True after OK or Cancel.
    public private(set) var closed = false
    var onClose: ((Bool) -> Void)?

    // Preview state.
    private var inFlight: UInt64?
    private var inFlightStart = Date()
    private var pending = false
    /// Preview renders sent (begin plus updates).
    public private(set) var rendersSent = 0
    /// Seconds from preview.begin to the first preview shown.
    public private(set) var firstPreviewSeconds: Double?
    /// Seconds each finished preview render took, in order.
    public private(set) var renderSeconds: [Double] = []
    /// True while the progress bar shows (a render past 200 ms).
    public var progressShown: Bool { !progress.isHidden }

    // Controls, by schema key (tests drive them).
    public private(set) var numberRows: [String: NumberRow] = [:]
    public private(set) var angleDials: [String: AngleDial] = [:]
    public private(set) var pointPads: [String: PointPad] = [:]
    public private(set) var checks: [String: NSButton] = [:]
    public private(set) var popups: [String: NSPopUpButton] = [:]
    public private(set) var textFields: [String: NSTextField] = [:]
    public private(set) var colorWells: [String: NSColorWell] = [:]
    public private(set) var curvesView: CurvesView?
    public private(set) var curvesChannel: NSSegmentedControl?
    public private(set) var levelsView: LevelsView?
    public private(set) var reseedButton: NSButton?
    public private(set) var okButton: NSButton!
    public private(set) var cancelButton: NSButton!
    public let progress = NSProgressIndicator()
    private var refreshers: [() -> Void] = []
    /// The first number or text field in schema order (focused when the sheet opens).
    private var firstField: NSTextField?

    /// The open dialog for each editor (one at a time).
    static var open: [ObjectIdentifier: EffectDialogController] = [:]
    public static func current(for editor: Editor) -> EffectDialogController? { open[ObjectIdentifier(editor)] }

    /// The last effect run from a menu or dialog, for Repeat (app-wide, as in paint.net).
    public internal(set) static var lastEffect: (id: String, label: String, values: [String: ParamValue])?
    /// The values each effect last ran with; its dialog opens with them.
    static var lastUsed: [String: [String: ParamValue]] = [:]

    static let width: CGFloat = 440
    static let labelWidth: CGFloat = 120
    static let margin: CGFloat = 16
    static let gap: CGFloat = 8
    var controlX: CGFloat { Self.margin + Self.labelWidth + 8 }
    var controlWidth: CGFloat { Self.width - controlX - Self.margin }

    public init(editor: Editor, schema: EffectSchema, values: [String: ParamValue]? = nil) {
        self.editor = editor
        self.schema = schema
        var v = schema.defaults
        for (k, x) in values ?? Self.lastUsed[schema.id] ?? [:] where v[k] != nil { v[k] = x }
        self.values = v
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: 200), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = schema.label
        super.init(window: panel)
        build(panel)
        refresh()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// The values as engine parameters.
    public var params: [String: any Sendable] { Self.json(values) }

    static func json(_ v: [String: ParamValue]) -> [String: any Sendable] { v.mapValues(\.json) }

    // MARK: Building

    private func build(_ panel: NSPanel) {
        let content = FilledView(fill: Theme.panel)
        panel.contentView = content
        var y: CGFloat = 20
        for control in schema.controls() {
            y = add(control, at: y, in: content) + Self.gap
        }
        y += 8
        // Bottom row: progress and Reseed on the left, Cancel and OK on the right.
        progress.style = .bar
        progress.isIndeterminate = true
        progress.controlSize = .small
        progress.isHidden = true
        progress.frame = NSRect(x: Self.margin, y: y + 8, width: 120, height: 12)
        progress.setAccessibilityLabel("Rendering preview")
        content.addSubview(progress)
        if schema.isSeeded {
            let b = NSButton(title: "Reseed", target: self, action: #selector(reseed(_:)))
            b.frame = NSRect(x: Self.margin + 128, y: y, width: 88, height: 28)
            b.toolTip = "Pick a new random seed"
            content.addSubview(b)
            reseedButton = b
        }
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        cancelButton.frame = NSRect(x: Self.width - Self.margin - 164, y: y, width: 80, height: 28)
        okButton = NSButton(title: "OK", target: self, action: #selector(ok(_:)))
        okButton.keyEquivalent = "\r"
        okButton.frame = NSRect(x: Self.width - Self.margin - 80, y: y, width: 80, height: 28)
        content.addSubview(cancelButton)
        content.addSubview(okButton)
        panel.setContentSize(NSSize(width: Self.width, height: y + 28 + 16))
    }

    private func label(_ text: String, y: CGFloat, height: CGFloat = 28, in view: NSView) {
        let l = Labels.label(text + ":", size: 12, color: Theme.ink)
        l.alignment = .right
        l.frame = NSRect(x: Self.margin, y: y + (min(height, 28) - 16) / 2, width: Self.labelWidth, height: 16)
        view.addSubview(l)
    }

    /// Adds one control at `y` and returns its bottom.
    private func add(_ control: EffectControl, at y: CGFloat, in view: NSView) -> CGFloat {
        switch control {
        case let .number(p):
            let title = EffectLabels.label(p.key)
            label(title, y: y, in: view)
            if p.isSeed {
                let row = NumberRow(min: p.min, max: p.max, step: 1, decimals: 0, width: 120, fieldWidth: 112,
                                    slider: false, label: title)
                row.frame.origin = NSPoint(x: controlX, y: y)
                row.onChange = { [weak self] v in self?.set(p.key, p.clamp(v)) }
                view.addSubview(row)
                numberRows[p.key] = row
                noteField(row.field)
                refreshers.append { [weak self] in self?.values[p.key]?.number.map(row.show) }
                return y + 28
            }
            let row = NumberRow(min: p.min, max: p.max, step: p.step, decimals: p.decimals, width: controlWidth,
                                label: title)
            row.frame.origin = NSPoint(x: controlX, y: y)
            row.onChange = { [weak self] v in self?.set(p.key, p.clamp(v)) }
            view.addSubview(row)
            numberRows[p.key] = row
            noteField(row.field)
            refreshers.append { [weak self] in self?.values[p.key]?.number.map(row.show) }
            return y + 28

        case let .angle(p):
            let title = EffectLabels.label(p.key)
            label(title, y: y + 14, in: view)
            let dial = AngleDial(min: p.min ?? -180, max: p.max ?? 180)
            dial.frame.origin = NSPoint(x: controlX, y: y)
            dial.setAccessibilityLabel(title)
            view.addSubview(dial)
            let row = NumberRow(min: p.min, max: p.max, step: p.step, decimals: p.decimals,
                                width: controlWidth - 64, label: title)
            row.frame.origin = NSPoint(x: controlX + 64, y: y + 14)
            view.addSubview(row)
            let degree = Labels.label("°", size: 12, color: Theme.muted)
            degree.frame = NSRect(x: Self.width - Self.margin + 2, y: y + 20, width: 12, height: 16)
            view.addSubview(degree)
            dial.onChange = { [weak self] v in self?.set(p.key, p.clamp(v)) }
            row.onChange = { [weak self] v in self?.set(p.key, p.clamp(v)) }
            angleDials[p.key] = dial
            numberRows[p.key] = row
            noteField(row.field)
            refreshers.append { [weak self] in
                guard let v = self?.values[p.key]?.number else { return }
                dial.degrees = v
                row.show(v)
            }
            return y + 56

        case let .point(px, py, title):
            label(title, y: y, in: view)
            let pad = PointPad(size: NSSize(width: 128, height: 96), x: (px.min ?? -1)...(px.max ?? 1),
                               y: (py.min ?? -1)...(py.max ?? 1))
            pad.frame.origin = NSPoint(x: controlX, y: y)
            pad.image = editor.thumbnail
            pad.setAccessibilityLabel(title)
            view.addSubview(pad)
            let fx = controlX + 128 + 12
            let rowX = NumberRow(min: px.min, max: px.max, step: px.step, decimals: px.decimals,
                                 width: 72, fieldWidth: 72, slider: false, label: "\(title) X")
            let rowY = NumberRow(min: py.min, max: py.max, step: py.step, decimals: py.decimals,
                                 width: 72, fieldWidth: 72, slider: false, label: "\(title) Y")
            for (i, (row, name)) in [(rowX, "X:"), (rowY, "Y:")].enumerated() {
                let l = Labels.label(name, size: 12, color: Theme.ink)
                l.frame = NSRect(x: fx, y: y + CGFloat(i) * 36 + 6, width: 16, height: 16)
                view.addSubview(l)
                row.frame.origin = NSPoint(x: fx + 20, y: y + CGFloat(i) * 36)
                view.addSubview(row)
            }
            pad.onChange = { [weak self] p in
                self?.set([px.key: px.clamp(p.x), py.key: py.clamp(p.y)])
            }
            rowX.onChange = { [weak self] v in self?.set(px.key, px.clamp(v)) }
            rowY.onChange = { [weak self] v in self?.set(py.key, py.clamp(v)) }
            pointPads[px.key] = pad
            numberRows[px.key] = rowX
            numberRows[py.key] = rowY
            noteField(rowX.field)
            refreshers.append { [weak self] in
                guard let self, let x = self.values[px.key]?.number, let y = self.values[py.key]?.number else { return }
                pad.point = CGPoint(x: x, y: y)
                rowX.show(x)
                rowY.show(y)
            }
            return y + 96

        case let .check(p):
            let b = NSButton(checkboxWithTitle: EffectLabels.label(p.key), target: self, action: #selector(checked(_:)))
            b.font = Theme.font(12)
            b.frame = NSRect(x: controlX - 2, y: y, width: controlWidth, height: 24)
            b.identifier = NSUserInterfaceItemIdentifier(p.key)
            view.addSubview(b)
            checks[p.key] = b
            refreshers.append { [weak self] in
                if case let .bool(on)? = self?.values[p.key] { b.state = on ? .on : .off }
            }
            return y + 24

        case let .popup(p):
            guard case let .choice(choices) = p.kind else { return y }
            label(EffectLabels.label(p.key), y: y, in: view)
            let popup = Labels.popup(choices.map(EffectLabels.label))
            popup.translatesAutoresizingMaskIntoConstraints = true
            popup.frame = NSRect(x: controlX - 2, y: y, width: min(controlWidth, 200), height: 28)
            popup.identifier = NSUserInterfaceItemIdentifier(p.key)
            popup.target = self
            popup.action = #selector(picked(_:))
            popup.setAccessibilityLabel(EffectLabels.label(p.key))
            view.addSubview(popup)
            popups[p.key] = popup
            refreshers.append { [weak self] in
                if case let .string(s)? = self?.values[p.key] { popup.selectItem(at: choices.firstIndex(of: s) ?? 0) }
            }
            return y + 28

        case let .text(p):
            label(EffectLabels.label(p.key), y: y, height: 24, in: view)
            let f = TextParamField(string: "")
            f.font = Theme.font(12)
            f.frame = NSRect(x: controlX, y: y, width: controlWidth, height: 24)
            f.setAccessibilityLabel(EffectLabels.label(p.key))
            f.onChange = { [weak self] s in self?.set(p.key, .string(s)) }
            view.addSubview(f)
            textFields[p.key] = f
            noteField(f)
            refreshers.append { [weak self] in
                if case let .string(s)? = self?.values[p.key], f.currentEditor() == nil { f.stringValue = s }
            }
            return y + 24

        case let .color(p):
            label(EffectLabels.label(p.key), y: y, in: view)
            let well = NSColorWell(frame: NSRect(x: controlX, y: y + 2, width: 44, height: 24))
            well.identifier = NSUserInterfaceItemIdentifier(p.key)
            well.target = self
            well.action = #selector(colorPicked(_:))
            well.setAccessibilityLabel(EffectLabels.label(p.key))
            view.addSubview(well)
            colorWells[p.key] = well
            refreshers.append { [weak self] in
                guard case let .bytes(b)? = self?.values[p.key], b.count >= 3 else { return }
                well.color = NSColor(srgbRed: CGFloat(b[0]) / 255, green: CGFloat(b[1]) / 255,
                                     blue: CGFloat(b[2]) / 255, alpha: 1)
            }
            return y + 28

        case let .curves(points, mode):
            var y = y
            let channel = NSSegmentedControl(labels: ["Red", "Green", "Blue"], trackingMode: .selectOne,
                                             target: self, action: #selector(curveChannelPicked(_:)))
            channel.selectedSegment = 0
            channel.setAccessibilityLabel("Curve channel")
            curvesChannel = channel
            if let mode, case let .choice(choices) = mode.kind {
                label(EffectLabels.label(mode.key), y: y, in: view)
                let popup = Labels.popup(choices.map { $0 == "Rgb" ? "RGB" : EffectLabels.label($0) })
                popup.translatesAutoresizingMaskIntoConstraints = true
                popup.frame = NSRect(x: controlX - 2, y: y, width: 130, height: 28)
                popup.identifier = NSUserInterfaceItemIdentifier(mode.key)
                popup.target = self
                popup.action = #selector(picked(_:))
                popup.setAccessibilityLabel(EffectLabels.label(mode.key))
                view.addSubview(popup)
                popups[mode.key] = popup
                channel.frame = NSRect(x: controlX + 136, y: y + 2, width: controlWidth - 136, height: 24)
                view.addSubview(channel)
                refreshers.append { [weak self] in
                    guard case let .string(s)? = self?.values[mode.key] else { return }
                    popup.selectItem(at: choices.firstIndex(of: s) ?? 0)
                    channel.isHidden = s != "Rgb"
                }
                y += 28 + Self.gap
            }
            let curves = CurvesView()
            curves.frame.origin = NSPoint(x: controlX, y: y)
            label(EffectLabels.label(points.key), y: y, in: view)
            curves.onChange = { [weak self] c in self?.set(points.key, .curve(c)) }
            view.addSubview(curves)
            curvesView = curves
            refreshers.append { [weak self] in
                guard case let .curve(c)? = self?.values[points.key], c != curves.channels else { return }
                curves.set(c)
            }
            let hint = Labels.label("Click to add a point. Option-click removes one.", size: 11, color: Theme.muted)
            hint.frame = NSRect(x: controlX, y: y + 256 + 4, width: controlWidth, height: 14)
            view.addSubview(hint)
            return y + 256 + 18

        case let .levels(ib, iw, g, ob, ow):
            let levels = LevelsView(width: Self.width - 2 * Self.margin, labelWidth: Self.labelWidth)
            levels.frame.origin = NSPoint(x: Self.margin, y: y)
            levels.onChange = { [weak self] v in
                self?.set([ib.key: .bytes(v.inputBlack), iw.key: .bytes(v.inputWhite), g.key: .gamma(v.gamma),
                           ob.key: .bytes(v.outputBlack), ow.key: .bytes(v.outputWhite)])
            }
            view.addSubview(levels)
            levelsView = levels
            noteField(levels.rows[0].field)
            refreshers.append { [weak self] in
                guard let self, let v = self.levelsValues(ib.key, iw.key, g.key, ob.key, ow.key),
                      v != levels.values else { return }
                levels.set(v)
            }
            return y + levels.frame.height
        }
    }

    func levelsValues(_ ib: String, _ iw: String, _ g: String, _ ob: String, _ ow: String) -> LevelsView.Values? {
        guard case let .bytes(a)? = values[ib], case let .bytes(b)? = values[iw], case let .gamma(c)? = values[g],
              case let .bytes(d)? = values[ob], case let .bytes(e)? = values[ow],
              [a, b, d, e].allSatisfy({ $0.count == 3 }), c.count == 3 else { return nil }
        return LevelsView.Values(inputBlack: a, inputWhite: b, gamma: c, outputBlack: d, outputWhite: e)
    }

    private func refresh() {
        for r in refreshers { r() }
    }

    private func noteField(_ f: NSTextField) {
        if firstField == nil { firstField = f }
    }

    // MARK: Control actions

    @objc private func checked(_ sender: NSButton) {
        guard let key = sender.identifier?.rawValue else { return }
        set(key, .bool(sender.state == .on))
    }

    @objc private func picked(_ sender: NSPopUpButton) {
        guard let key = sender.identifier?.rawValue, let p = schema.param(key), case let .choice(c) = p.kind,
              sender.indexOfSelectedItem >= 0, sender.indexOfSelectedItem < c.count else { return }
        set(key, .string(c[sender.indexOfSelectedItem]))
    }

    @objc private func colorPicked(_ sender: NSColorWell) {
        guard let key = sender.identifier?.rawValue, let c = sender.color.usingColorSpace(.sRGB) else { return }
        var bytes = [c.redComponent, c.greenComponent, c.blueComponent].map { Int(($0 * 255).rounded()) }
        if case let .bytes(old)? = values[key], old.count == 4 { bytes.append(old[3]) }
        set(key, .bytes(bytes))
    }

    @objc private func curveChannelPicked(_ sender: NSSegmentedControl) {
        curvesView?.editing = max(sender.selectedSegment, 0)
    }

    /// Picks a new random seed for every seed field.
    @objc public func reseed(_ sender: Any?) {
        var changes: [String: ParamValue] = [:]
        for p in schema.params where p.isSeed {
            let hi = Int(p.max ?? Double(Int32.max)), lo = Int(p.min ?? 0)
            var s = Int.random(in: lo...max(lo, hi))
            if case let .int(old)? = values[p.key], hi > lo { while s == old { s = Int.random(in: lo...hi) } }
            changes[p.key] = .int(s)
        }
        set(changes)
    }

    // MARK: Values

    /// Sets one value and updates the preview (also used by tests).
    public func set(_ key: String, _ value: ParamValue) { set([key: value]) }

    /// Sets several values at once (one preview update).
    public func set(_ changes: [String: ParamValue]) {
        guard !closed else { return }
        var next = values
        for (k, v) in changes where next[k] != nil { next[k] = v }
        Self.fixCurves(&next, schema: schema)
        guard next != values else { return }
        values = next
        refresh()
        schedulePreview()
    }

    /// Curves in RGB mode need three channels and in Luminosity mode one: a mode
    /// change copies the edited curve to the new channel count.
    static func fixCurves(_ v: inout [String: ParamValue], schema: EffectSchema) {
        guard case let .string(mode)? = v["ColorTransferMode"], case let .curve(c)? = v["ControlPoints"],
              !c.isEmpty else { return }
        let want = mode == "Rgb" ? 3 : 1
        if c.count != want { v["ControlPoints"] = .curve(Array(repeating: c[0], count: want)) }
    }

    // MARK: Preview

    /// Opens the dialog as a sheet on the editor's window and starts the preview.
    public func present(on window: NSWindow) {
        Self.open[ObjectIdentifier(editor)] = self
        L10n.localizeDialog(self.window!)  // M59: translated and named before it shows
        window.beginSheet(self.window!) { _ in }
        self.window?.initialFirstResponder = firstField
        if let firstField { self.window?.makeFirstResponder(firstField) }
        startPreview()
    }

    /// Sends preview.begin with the current values.
    func startPreview() {
        guard !sessionStarted, !closed else { return }
        sessionStarted = true
        send(begin: true)
    }

    private func schedulePreview() {
        guard sessionStarted, !closed else { return }
        if inFlight != nil {
            pending = true
        } else {
            send(begin: false)
        }
    }

    private func send(begin: Bool) {
        pending = false
        let ticket: UInt64
        do {
            ticket = begin
                ? try editor.document.dispatch("preview.begin", ["command": schema.id, "params": params])
                : try editor.document.dispatch("preview.update", params)
        } catch {
            editor.report("\(schema.label) preview failed: \(error)")
            return
        }
        rendersSent += 1
        inFlight = ticket
        inFlightStart = Date()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            guard let self, self.inFlight == ticket, !self.closed else { return }
            self.progress.isHidden = false
            self.progress.startAnimation(nil)
        }
        let engine = editor.document.engine
        Task { @MainActor [weak self] in
            let event = await engine.result(of: ticket, timeout: .seconds(120))
            self?.finished(ticket, begin: begin, event)
        }
    }

    private func finished(_ ticket: UInt64, begin: Bool, _ event: EngineEvent?) {
        guard inFlight == ticket else { return }
        inFlight = nil
        progress.stopAnimation(nil)
        progress.isHidden = true
        progress.isIndeterminate = true
        let status = event?.status ?? "timeout"
        if status == "ok" {
            let s = Date().timeIntervalSince(inFlightStart)
            renderSeconds.append(s)
            if firstPreviewSeconds == nil { firstPreviewSeconds = s }
        } else if status != "dropped" && status != "cancelled" && !closed {
            editor.report("\(schema.label) preview failed: \(status)")
        }
        if pending, !closed { send(begin: false) }
    }

    /// Engine progress for a running preview render (from the workspace's
    /// event loop).
    static func progressEvent(_ event: EngineEvent) {
        guard let dialog = open.values.first(where: { $0.inFlight == event.ticket }) else { return }
        let j = event.json
        if let done = (j["done"] as? NSNumber)?.doubleValue, let total = (j["total"] as? NSNumber)?.doubleValue,
           total > 0 {
            dialog.progress.isIndeterminate = false
            dialog.progress.doubleValue = done / total * 100
        }
    }

    /// True when no render is running and no change waits to be sent.
    public var isIdle: Bool { inFlight == nil && !pending }

    // MARK: OK and Cancel

    @objc public func ok(_ sender: Any?) {
        guard !closed else { return }
        closed = true
        let stale = inFlight
        if sessionStarted {
            if pending {
                // The newest values never reached the session: end it and run
                // the effect once with them (the same single history step).
                editor.run("preview.cancel")
                editor.run(schema.id, params)
            } else {
                editor.run("preview.commit")
            }
        } else {
            editor.run(schema.id, params)
        }
        if let stale { try? editor.document.engine.cancel(ticket: stale) }
        Self.lastUsed[schema.id] = values
        Self.lastEffect = (schema.id, schema.label, values)
        close(true)
    }

    @objc public func cancel(_ sender: Any?) {
        guard !closed else { return }
        closed = true
        if let t = inFlight { try? editor.document.engine.cancel(ticket: t) }
        if sessionStarted { editor.run("preview.cancel") }
        close(false)
    }

    private func close(_ ok: Bool) {
        pending = false
        progress.stopAnimation(nil)
        Self.open[ObjectIdentifier(editor)] = nil
        if let sheet = window, let parent = sheet.sheetParent {
            parent.endSheet(sheet)
        }
        window?.orderOut(nil)
        onClose?(ok)
    }
}

/// A text field that reports every edit.
final class TextParamField: NSTextField, NSTextFieldDelegate {
    var onChange: ((String) -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        delegate = self
    }

    convenience init(string: String) {
        self.init(frame: .zero)
        stringValue = string
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    func controlTextDidChange(_ note: Notification) { onChange?(stringValue) }
}

// MARK: Menu actions

/// Effects menu selectors the document window controller implements.
@MainActor @objc public protocol EffectMenuActions {
    func repeatEffect(_ sender: Any?)
}

/// The target of every Adjustments and Effects menu item: it opens the item's
/// dialog on the main document window (or runs it at once when the effect has
/// no parameters). `EditorWindowController.runEffect(_:)` stays the direct
/// "run with defaults" path that scripts and tests call.
@MainActor
public final class EffectMenuTarget: NSObject, NSMenuItemValidation {
    public static let shared = EffectMenuTarget()

    var controller: EditorWindowController? {
        (NSApp.mainWindow?.windowController ?? NSApp.keyWindow?.windowController) as? EditorWindowController
    }

    @objc public func runEffect(_ sender: Any?) {
        guard let id = (sender as? NSMenuItem)?.representedObject as? String else { return }
        controller?.showEffect(id)
    }

    public func validateMenuItem(_ item: NSMenuItem) -> Bool {
        controller?.validateMenuItem(item) ?? false
    }
}

extension EditorWindowController: EffectMenuActions {
    /// Opens the effect's dialog, or runs it at once when it has no parameters.
    /// Returns the dialog when one opened.
    @discardableResult
    public func showEffect(_ id: String) -> EffectDialogController? {
        guard let window, window.attachedSheet == nil else { return nil }
        let schema: EffectSchema
        do {
            guard let s = try EffectCatalog.schema(id, engine: editor.document.engine) else {
                editor.report("unknown effect \(id)")
                return nil
            }
            schema = s
        } catch {
            editor.report("could not read the effect catalog: \(error)")
            return nil
        }
        guard schema.hasParameters else {
            editor.run(id)
            EffectDialogController.lastEffect = (id, schema.label, [:])
            return nil
        }
        let dialog = EffectDialogController(editor: editor, schema: schema)
        dialog.present(on: window)
        return dialog
    }

    /// Effects > Repeat: runs the last effect again with the same values and
    /// seed, without a dialog.
    @objc public func repeatEffect(_ sender: Any?) {
        guard let last = EffectDialogController.lastEffect else { return }
        editor.run(last.id, EffectDialogController.json(last.values))
    }

    /// Enables Repeat once an effect has run and names it, as paint.net does
    /// ("Repeat Gaussian Blur").
    func validateRepeat(_ item: NSMenuItem, busy: Bool) -> Bool {
        guard let last = EffectDialogController.lastEffect else {
            item.title = "Repeat Effect"
            item.image = MenuIcons.image(for: "Effects > Repeat Effect")
            return false
        }
        item.title = "Repeat \(last.label)"
        // paint.net shows the last effect's icon on Repeat (M55).
        item.image = MenuIcons.effectImage(last.id) ?? MenuIcons.image(for: "Effects > Repeat Effect")
        return !busy && editor.isEnabled(last.id)
    }
}
