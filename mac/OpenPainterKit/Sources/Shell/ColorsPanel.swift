// The Colors panel (M36), after paint.net's Colors window
// (https://www.getpaint.net/doc/latest/ColorsWindow.html) and the Classic v2
// spec (docs/design/SPEC.md, "Colors and state"):
//
// - Primary and secondary swatches. The active slot has the notch; clicking
//   the other swatch moves the notch. Swap (X) and reset to black and white.
// - The hex field and the HSV wheel edit the active slot. A wheel right-click
//   sets the other slot. Wheel keys: Command (paint.net Ctrl) keeps the radius,
//   Option (Alt) keeps the spoke, Shift snaps to 15 degree spokes,
//   Command-Shift snaps on the same radius. The ring marker shows the color.
// - More » / « Less shows RGB, HSV and alpha sliders with fields.
// - The palette: 32 swatches compact, all 96 when expanded (scrolling).
//   Click sets the active slot; right-click, Control-click or Shift-click sets
//   the other slot. The palette menu loads, saves and resets palettes.
//
// Swatch and palette bytes are document colors: drawn as exact sRGB in both
// appearances. Only frames, text and markers follow the theme.
import AppKit
import UniformTypeIdentifiers

@MainActor
public final class ColorsPanelView: NSView {
    let tools: ToolState
    public let swatches = SwatchPair()
    public private(set) var swapButton: ShellButton!
    public private(set) var resetButton: ShellButton!
    public let hexField = Labels.field("", width: 72)
    private let hexLabel = Labels.label("Hex")
    public let wheel = ColorWheelView()
    public private(set) var moreButton: NSButton!
    public private(set) var addColorButton: ToggleButton!
    public private(set) var paletteButton: ShellButton!
    public let palette = PaletteView()
    public private(set) var channels: [Channel: ChannelRow] = [:]
    /// True when More is on (sliders and the whole palette shown).
    public private(set) var isExpanded = false
    /// Called when More or Less changes the panel's wanted height.
    public var onExpandedChange: ((Bool) -> Void)?
    /// Called with a palette load or save report (the status bar shows it).
    public var onReport: ((String) -> Void)?
    /// The last palette report, for the panel tooltip and tests.
    public private(set) var lastReport: String?
    /// The HSV of the active color. Kept across edits so a grey or black
    /// color does not lose its hue and saturation.
    public private(set) var hsv = HSV(h: 0, s: 0, v: 0)
    /// The text the hex field last showed.
    private var shownHex = ""

    public enum Channel: Int, CaseIterable, Sendable {
        case red, green, blue, hue, saturation, value, alpha
        var label: String { ["R", "G", "B", "H", "S", "V", "A"][rawValue] }
        var name: String { ["Red", "Green", "Blue", "Hue", "Saturation", "Value", "Alpha"][rawValue] }
        var range: ClosedRange<Double> {
            switch self {
            case .hue: 0...360
            case .saturation, .value: 0...100
            default: 0...255
            }
        }
    }

    static let pad: CGFloat = 10
    static let rowY: CGFloat = 72
    static let moreY: CGFloat = 102
    static let channelRow: CGFloat = 26

    public init(tools: ToolState) {
        self.tools = tools
        super.init(frame: NSRect(x: 0, y: 0, width: 244, height: 222))
        swatches.tools = tools
        swatches.onPick = { [weak self] slot in self?.tools.activeSlot = slot }
        addSubview(swatches)
        swapButton = button("arrow.left.arrow.right", "Swap colors", "Swap colors (X)", #selector(swap(_:)))
        resetButton = button("circle.lefthalf.filled", "Reset colors", "Black and white", #selector(reset(_:)))
        hexLabel.frame = .zero
        addSubview(hexLabel)
        hexField.translatesAutoresizingMaskIntoConstraints = true
        for c in hexField.constraints { c.isActive = false }
        hexField.alignment = .left
        hexField.target = self
        hexField.action = #selector(hexEntered(_:))
        hexField.toolTip = "RRGGBB, or RRGGBBAA with alpha"
        addSubview(hexField)
        wheel.onPick = { [weak self] dx, dy, mods, other in self?.wheelPicked(dx: dx, dy: dy, mods: mods, other: other) }
        addSubview(wheel)
        moreButton = NSButton(title: "More »", target: self, action: #selector(toggleMore(_:)))
        moreButton.bezelStyle = .rounded
        moreButton.controlSize = .small
        moreButton.font = Theme.font(12)
        moreButton.setAccessibilityLabel("More")
        addSubview(moreButton)
        addColorButton = ToggleButton(symbol: "plus.square", label: "Add color to palette",
                                      tooltip: "Add the active color: then click a palette swatch to replace it",
                                      size: 24, pointSize: 13, target: self, action: #selector(addColorToggled(_:)))
        addColorButton.translatesAutoresizingMaskIntoConstraints = true
        for c in addColorButton.constraints { c.isActive = false }
        addSubview(addColorButton)
        paletteButton = button("ellipsis.circle", "Palette menu", "Load, save or reset the palette",
                               #selector(showPaletteMenu(_:)))
        palette.onPick = { [weak self] index, other in self?.palettePicked(index, other: other) }
        addSubview(palette)
        for ch in Channel.allCases {
            let row = ChannelRow(channel: ch)
            row.onChange = { [weak self] v in self?.channelChanged(ch, v) }
            row.isHidden = true
            channels[ch] = row
            addSubview(row)
        }
        tools.observe(self) { [weak self] in self?.sync() }
        sync()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func button(_ symbol: String, _ label: String, _ tip: String, _ action: Selector) -> ShellButton {
        let b = ShellButton(symbol: symbol, label: label, tooltip: tip, size: 24, pointSize: 13, target: self,
                            action: action)
        b.translatesAutoresizingMaskIntoConstraints = true
        for c in b.constraints { c.isActive = false }
        addSubview(b)
        return b
    }

    public override var isFlipped: Bool { true }

    public override func draw(_ dirtyRect: NSRect) {
        Theme.panel.setFill()
        bounds.fill()
    }

    /// The content height that shows the compact body (wheel row, buttons,
    /// 4 palette rows) with 8 pt below.
    public static var compactContentHeight: CGFloat { moreY + PaletteView.height(rows: 4) + 8 }

    public override func layout() {
        super.layout()
        let p = Self.pad
        swatches.frame = NSRect(x: p, y: p, width: 54, height: 54)
        swapButton.frame = NSRect(x: 68, y: p, width: 24, height: 24)
        resetButton.frame = NSRect(x: 68, y: p + 30, width: 24, height: 24)
        hexLabel.frame = NSRect(x: 100, y: p + 1, width: 72, height: 15)
        hexField.frame = NSRect(x: 100, y: p + 18, width: 72, height: 24)
        wheel.frame = NSRect(x: bounds.width - p - 52, y: p, width: 52, height: 52)
        // M59: wider when a translation needs it, up to the add-color button.
        let moreWidth = L10n.adjustsLayout
            ? max(84, min((moreButton.cell?.cellSize.width ?? 84).rounded(.up), bounds.width - 2 * p - 60)) : 84
        moreButton.frame = NSRect(x: p - 2, y: Self.rowY, width: moreWidth, height: 24)
        paletteButton.frame = NSRect(x: bounds.width - p - 24, y: Self.rowY, width: 24, height: 24)
        addColorButton.frame = NSRect(x: bounds.width - p - 52, y: Self.rowY, width: 24, height: 24)
        var y = Self.moreY
        for ch in Channel.allCases {
            channels[ch]!.frame = NSRect(x: p, y: y, width: bounds.width - 2 * p, height: 24)
            y += Self.channelRow
        }
        let paletteY = isExpanded ? y + 6 : Self.moreY
        palette.frame = NSRect(x: 0, y: paletteY, width: bounds.width, height: max(bounds.height - paletteY - 8, 0))
    }

    // MARK: State to views

    func sync() {
        let c = tools.color(tools.activeSlot)
        if hsv.color(alpha: c.a) != c {
            var n = HSV(c)
            if n.s == 0 || n.v == 0 { n.h = hsv.h }  // grey has no hue: keep the last one
            if n.v == 0 { n.s = hsv.s }
            hsv = n
        }
        swatches.needsDisplay = true
        swatches.setAccessibilityValue(
            "primary \(tools.primary.hex), secondary \(tools.secondary.hex), \(tools.activeSlot == .primary ? "primary" : "secondary") active")
        if hexField.currentEditor() == nil {
            hexField.stringValue = c.hex
            shownHex = c.hex
        }
        hexField.setVoiceOverLabel(tools.activeSlot == .primary  // M59: translated, and on the cell VoiceOver reads
            ? L10n.tr("Primary color hex", "Colors panel: VoiceOver label") : L10n.tr("Secondary color hex", "Colors panel: VoiceOver label"))
        wheel.hsv = hsv
        let values: [Channel: Double] = [
            .red: Double(c.r), .green: Double(c.g), .blue: Double(c.b), .hue: hsv.h.rounded(),
            .saturation: hsv.s.rounded(), .value: hsv.v.rounded(), .alpha: Double(c.a),
        ]
        for (ch, row) in channels {
            row.set(value: values[ch]!, gradient: gradient(ch, c))
        }
        palette.colors = Array(tools.palette.colors.prefix(isExpanded ? Palette.size : Palette.compactCount))
        palette.chosen = tools.palette.colors.firstIndex(of: c).flatMap { $0 < palette.colors.count ? $0 : nil }
        palette.addMode = addColorButton.isOn
    }

    /// The colors along a slider's track for the current color.
    private func gradient(_ ch: Channel, _ c: DocColor) -> [DocColor] {
        (0...16).map { i in
            let t = Double(i) / 16
            var d = c
            d.a = 255
            switch ch {
            case .red: d.r = UInt8((t * 255).rounded())
            case .green: d.g = UInt8((t * 255).rounded())
            case .blue: d.b = UInt8((t * 255).rounded())
            case .hue: d = HSV(h: t * 360, s: 100, v: 100).color()
            case .saturation: d = HSV(h: hsv.h, s: t * 100, v: max(hsv.v, 1)).color()
            case .value: d = HSV(h: hsv.h, s: hsv.s, v: t * 100).color()
            case .alpha: d.a = UInt8((t * 255).rounded())
            }
            return d
        }
    }

    // MARK: Edits

    /// Sets the active slot (or the other one) to `c`, keeping `newHSV` as the
    /// panel's HSV when it gives exactly these bytes.
    private func apply(_ c: DocColor, to slot: ColorSlot, hsv newHSV: HSV? = nil) {
        if slot == tools.activeSlot, let newHSV { hsv = newHSV }
        tools.set(slot, c)
        sync()
    }

    @objc private func swap(_ sender: Any?) { tools.swapColors() }
    @objc private func reset(_ sender: Any?) { tools.resetColors() }

    @objc private func hexEntered(_ sender: NSTextField) {
        let text = sender.stringValue
        // Ending an edit that changed nothing must not write the text back: the
        // color may have changed elsewhere while the field kept its old text.
        if text == shownHex {
            // No change.
        } else if let c = DocColor(hex: text) {
            apply(c, to: tools.activeSlot)
        } else if !text.trimmingCharacters(in: .whitespaces).isEmpty {
            NSSound.beep()
        }
        sender.stringValue = tools.color(tools.activeSlot).hex
        shownHex = sender.stringValue
        // Give the keyboard back to the document window.
        sender.window?.makeFirstResponder(nil)
    }

    func wheelPicked(dx: Double, dy: Double, mods: ColorWheelMath.Modifiers, other: Bool) {
        let slot = other ? tools.activeSlot.other : tools.activeSlot
        let base = other ? HSV(tools.color(slot)) : hsv
        let (h, s) = ColorWheelMath.pick(dx: dx, dy: dy, current: base, mods: mods)
        // The wheel picks hue and saturation; value stays, except that black
        // would hide the pick, so value 0 becomes 100.
        let n = HSV(h: h, s: s, v: base.v == 0 ? 100 : base.v)
        apply(n.color(alpha: tools.color(slot).a), to: slot, hsv: n)
    }

    func channelChanged(_ ch: Channel, _ v: Double) {
        let slot = tools.activeSlot
        var c = tools.color(slot)
        let byte = UInt8(min(max(v.rounded(), 0), 255))
        switch ch {
        case .red: c.r = byte
        case .green: c.g = byte
        case .blue: c.b = byte
        case .alpha: c.a = byte
        case .hue, .saturation, .value:
            var n = hsv
            if ch == .hue { n.h = v >= 360 ? 0 : v }
            if ch == .saturation { n.s = v }
            if ch == .value { n.v = v }
            return apply(n.color(alpha: c.a), to: slot, hsv: n)
        }
        apply(c, to: slot)
    }

    func palettePicked(_ index: Int, other: Bool) {
        guard index >= 0, index < tools.palette.colors.count else { return }
        if addColorButton.isOn {
            // Add mode: put the active color into the clicked slot.
            var p = tools.palette
            p.colors[index] = tools.color(tools.activeSlot)
            tools.palette = p
            addColorButton.isOn = false
            sync()
            return
        }
        apply(tools.palette.colors[index], to: other ? tools.activeSlot.other : tools.activeSlot)
    }

    @objc private func addColorToggled(_ sender: Any?) { sync() }

    @objc public func toggleMore(_ sender: Any?) { setExpanded(!isExpanded) }

    public func setExpanded(_ on: Bool) {
        guard on != isExpanded else { return }
        isExpanded = on
        moreButton.title = on ? L10n.tr("« Less", "Colors panel: button title") : L10n.tr("More »", "Colors panel: button title")
        moreButton.setVoiceOverLabel(on ? L10n.tr("Less", "Colors panel: VoiceOver label") : L10n.tr("More", "Colors panel: VoiceOver label"))
        for row in channels.values { row.isHidden = !on }
        sync()
        needsLayout = true
        onExpandedChange?(on)
    }

    // MARK: Palette files

    @objc private func showPaletteMenu(_ sender: NSView) {
        let menu = NSMenu()
        menu.addItem(withTitle: "Load Palette…", action: #selector(loadPaletteChosen(_:)), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Save Palette As…", action: #selector(savePaletteChosen(_:)), keyEquivalent: "").target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: "Reset to Default Palette", action: #selector(resetPalette(_:)), keyEquivalent: "").target = self
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 2), in: sender)
    }

    @objc private func loadPaletteChosen(_ sender: Any?) {
        let open = NSOpenPanel()
        open.allowedContentTypes = [.plainText]
        open.allowsMultipleSelection = false
        guard open.runModal() == .OK, let url = open.url else { return }
        loadPalette(from: url)
    }

    @objc private func savePaletteChosen(_ sender: Any?) {
        let save = NSSavePanel()
        save.allowedContentTypes = [.plainText]
        save.nameFieldStringValue = "Palette.txt"
        guard save.runModal() == .OK, let url = save.url else { return }
        savePalette(to: url)
    }

    @objc private func resetPalette(_ sender: Any?) {
        tools.palette = .standard
        report("Palette reset to the default.")
    }

    /// Loads a paint.net palette file. Malformed lines are skipped and reported.
    @discardableResult
    public func loadPalette(from url: URL) -> Palette.ParseResult? {
        let text: String
        do {
            let data = try Data(contentsOf: url)
            guard let t = String(data: data, encoding: .utf8) ?? String(data: data, encoding: .isoLatin1) else {
                report("Could not read \(url.lastPathComponent) as text.")
                return nil
            }
            text = t
        } catch {
            report("Could not open \(url.lastPathComponent): \(error.localizedDescription)")
            return nil
        }
        let result = Palette.parse(text)
        if result.colorCount == 0 {
            report("\(url.lastPathComponent) has no palette colors; the palette did not change. \(result.summary)")
            return result
        }
        tools.palette = result.palette
        report("\(url.lastPathComponent): \(result.summary)")
        return result
    }

    /// Saves the palette as a paint.net palette file.
    @discardableResult
    public func savePalette(to url: URL) -> Bool {
        do {
            try tools.palette.fileText.write(to: url, atomically: true, encoding: .utf8)
            report("Saved the palette to \(url.lastPathComponent).")
            return true
        } catch {
            report("Could not save the palette: \(error.localizedDescription)")
            return false
        }
    }

    private func report(_ text: String) {
        lastReport = text
        palette.toolTip = text
        onReport?(text)
    }
}

// MARK: Swatches

/// The primary (top-left) and secondary (bottom-right) swatches. The notch
/// marks the active slot. Clicking the inactive swatch makes it active.
public final class SwatchPair: NSView {
    weak var tools: ToolState?
    var onPick: ((ColorSlot) -> Void)?
    static let primaryRect = NSRect(x: 0, y: 0, width: 32, height: 32)
    static let secondaryRect = NSRect(x: 22, y: 22, width: 32, height: 32)

    public override var isFlipped: Bool { true }

    public override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Primary and secondary colors")
        setAccessibilityHelp("Press to make the other color active")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func draw(_ dirtyRect: NSRect) {
        guard let tools else { return }
        for (r, slot) in [(Self.secondaryRect, ColorSlot.secondary), (Self.primaryRect, .primary)] {
            ColorDrawing.swatch(tools.color(slot), in: r)
            Theme.panelLine.setStroke()
            NSBezierPath(rect: r.insetBy(dx: 0.5, dy: 0.5)).stroke()
            if slot == tools.activeSlot {
                // The notch: a frame-colored corner wedge on the active slot.
                let n = NSBezierPath()
                n.move(to: r.origin)
                n.line(to: NSPoint(x: r.minX + 9, y: r.minY))
                n.line(to: NSPoint(x: r.minX, y: r.minY + 9))
                n.close()
                Theme.ink.setFill()
                n.fill()
            }
        }
    }

    /// The slot under a point (the primary square is on top).
    func slot(at p: NSPoint) -> ColorSlot? {
        if Self.primaryRect.contains(p) { return .primary }
        if Self.secondaryRect.contains(p) { return .secondary }
        return nil
    }

    public override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if let s = slot(at: p) { onPick?(s) }
    }

    public override func accessibilityPerformPress() -> Bool {
        guard let tools else { return false }
        onPick?(tools.activeSlot.other)
        return true
    }

    // M59: Keyboard navigation reaches the swatches; Space or Return swaps the active slot.
    public override var acceptsFirstResponder: Bool { KeyboardAccess.enabled }
    public override var canBecomeKeyView: Bool { KeyboardAccess.enabled }
    public override var focusRingMaskBounds: NSRect { Self.primaryRect.union(Self.secondaryRect) }
    public override func drawFocusRingMask() { Self.primaryRect.union(Self.secondaryRect).fill() }
    public override func keyDown(with event: NSEvent) {
        if KeyboardAccess.isPress(event) { _ = accessibilityPerformPress() } else { super.keyDown(with: event) }
    }
}

/// Exact document-color drawing: the bytes go out as sRGB in both appearances.
/// A color with alpha below 255 shows over a small checkerboard.
enum ColorDrawing {
    static func swatch(_ c: DocColor, in r: NSRect) {
        if c.a < 255 {
            NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1).setFill()
            r.fill()
            NSColor(srgbRed: 0.8, green: 0.8, blue: 0.8, alpha: 1).setFill()
            let s: CGFloat = 6
            var y = r.minY
            var row = 0
            while y < r.maxY {
                var x = r.minX + (row % 2 == 0 ? 0 : s)
                while x < r.maxX {
                    NSRect(x: x, y: y, width: min(s, r.maxX - x), height: min(s, r.maxY - y)).fill()
                    x += 2 * s
                }
                y += s
                row += 1
            }
        }
        c.nsColor.setFill()
        r.fill(using: .sourceOver)
    }
}

// MARK: Wheel

/// The HSV wheel: hue around, saturation outward, drawn at full value. The
/// ring marker shows the active color's hue and saturation.
public final class ColorWheelView: NSView {
    public var hsv = HSV(h: 0, s: 0, v: 100) { didSet { if hsv != oldValue { needsDisplay = true } } }
    /// (dx, dy in radius units with y up, modifiers, right-click).
    var onPick: ((Double, Double, ColorWheelMath.Modifiers, Bool) -> Void)?
    private var image: CGImage?
    private var imageSize = 0
    private var rightDrag = false

    public override var isFlipped: Bool { true }

    public override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.colorWell)
        setAccessibilityLabel("Color wheel")
        toolTip = "Click: active color. Right-click: the other color. ⌘ keeps saturation, ⌥ keeps hue, ⇧ snaps to 15° spokes."
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func draw(_ dirtyRect: NSRect) {
        let scale = window?.backingScaleFactor ?? 2
        let px = Int((bounds.width * scale).rounded())
        if image == nil || imageSize != px {
            image = Self.render(size: px)
            imageSize = px
        }
        if let image, let ctx = NSGraphicsContext.current?.cgContext {
            ctx.saveGState()
            // The image rows go top to bottom; this view is flipped.
            ctx.translateBy(x: 0, y: bounds.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(image, in: CGRect(origin: .zero, size: bounds.size))
            ctx.restoreGState()
        }
        Theme.panelLine.setStroke()
        NSBezierPath(ovalIn: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()
        // The ring marker: a dark ring inside a light ring, visible on any hue.
        let (mx, my) = ColorWheelMath.point(for: hsv)
        let r = bounds.width / 2
        let c = NSPoint(x: bounds.midX + mx * r, y: bounds.midY - my * r)
        let outer = NSBezierPath(ovalIn: NSRect(x: c.x - 5, y: c.y - 5, width: 10, height: 10))
        outer.lineWidth = 2
        NSColor.white.setStroke()
        outer.stroke()
        let inner = NSBezierPath(ovalIn: NSRect(x: c.x - 3.5, y: c.y - 3.5, width: 7, height: 7))
        inner.lineWidth = 1.5
        NSColor.black.setStroke()
        inner.stroke()
    }

    /// A square sRGB image of the disc, rows from the top.
    static func render(size: Int) -> CGImage? {
        guard size > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: size * size * 4)
        let r = Double(size) / 2
        for y in 0..<size {
            for x in 0..<size {
                let dx = (Double(x) + 0.5 - r) / r, dy = (r - Double(y) - 0.5) / r
                let d = hypot(dx, dy)
                guard d <= 1 + 1 / r else { continue }
                var h = atan2(dy, dx) * 180 / .pi
                if h < 0 { h += 360 }
                let c = HSV(h: h, s: min(d, 1) * 100, v: 100).color()
                // Antialias the rim; premultiplied.
                let a = min(max((1 - d) * r + 0.5, 0), 1)
                let i = (y * size + x) * 4
                bytes[i] = UInt8((Double(c.r) * a).rounded())
                bytes[i + 1] = UInt8((Double(c.g) * a).rounded())
                bytes[i + 2] = UInt8((Double(c.b) * a).rounded())
                bytes[i + 3] = UInt8((a * 255).rounded())
            }
        }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(
            width: size, height: size, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: size * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue), provider: provider,
            decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }

    /// Mac keys to the paint.net wheel modifiers (Command stands for Ctrl).
    static func modifiers(_ flags: NSEvent.ModifierFlags) -> ColorWheelMath.Modifiers {
        var m: ColorWheelMath.Modifiers = []
        if flags.contains(.command) { m.insert(.keepRadius) }
        if flags.contains(.option) { m.insert(.keepSpoke) }
        if flags.contains(.shift) { m.insert(.snapSpokes) }
        return m
    }

    private func pick(_ event: NSEvent, other: Bool) {
        let p = convert(event.locationInWindow, from: nil)
        let r = bounds.width / 2
        onPick?((p.x - bounds.midX) / r, (bounds.midY - p.y) / r, Self.modifiers(event.modifierFlags), other)
    }

    public override func mouseDown(with event: NSEvent) {
        rightDrag = event.modifierFlags.contains(.control)
        pick(event, other: rightDrag)
    }

    public override func mouseDragged(with event: NSEvent) { pick(event, other: rightDrag) }

    public override func rightMouseDown(with event: NSEvent) {
        rightDrag = true
        pick(event, other: true)
    }

    public override func rightMouseDragged(with event: NSEvent) { pick(event, other: true) }

    public override func accessibilityValue() -> Any? {
        "hue \(Int(hsv.h.rounded())), saturation \(Int(hsv.s.rounded()))"
    }

    // M59: Keyboard navigation reaches the wheel. Left and right turn the hue
    // by 1 degree (Shift: 15), up and down change the saturation by 1 (Shift: 10).
    public override var acceptsFirstResponder: Bool { KeyboardAccess.enabled }
    public override var canBecomeKeyView: Bool { KeyboardAccess.enabled }
    public override var focusRingMaskBounds: NSRect { bounds }
    public override func drawFocusRingMask() { NSBezierPath(ovalIn: bounds).fill() }
    public override func keyDown(with event: NSEvent) {
        let big = event.modifierFlags.contains(.shift)
        var h = hsv.h, s = hsv.s
        switch event.keyCode {
        case 123: h -= big ? 15 : 1
        case 124: h += big ? 15 : 1
        case 125: s -= big ? 10 : 1
        case 126: s += big ? 10 : 1
        default: return super.keyDown(with: event)
        }
        h = (h.truncatingRemainder(dividingBy: 360) + 360).truncatingRemainder(dividingBy: 360)
        s = min(max(s, 0), 100)
        let a = h * .pi / 180
        onPick?(cos(a) * s / 100, sin(a) * s / 100, [], false)
    }
}

// MARK: Sliders

/// One More row: a label, a gradient slider and a number field.
@MainActor
public final class ChannelRow: NSView, NSTextFieldDelegate {
    public let channel: ColorsPanelView.Channel
    public let slider: GradientSlider
    public let field = Labels.field("0", width: 44)
    private let label: NSTextField
    var onChange: ((Double) -> Void)?

    init(channel: ColorsPanelView.Channel) {
        self.channel = channel
        slider = GradientSlider(range: channel.range)
        label = Labels.label(channel.label, size: 12, color: Theme.muted, weight: .semibold)
        super.init(frame: .zero)
        addSubview(label)
        slider.setAccessibilityLabel(channel.name)
        slider.checkered = channel == .alpha
        slider.onChange = { [weak self] v in self?.onChange?(v) }
        addSubview(slider)
        field.translatesAutoresizingMaskIntoConstraints = true
        for c in field.constraints { c.isActive = false }
        field.target = self
        field.action = #selector(entered(_:))
        field.setAccessibilityLabel(channel.name)
        addSubview(field)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    public override func layout() {
        super.layout()
        label.frame = NSRect(x: 0, y: 4, width: 14, height: 16)
        field.frame = NSRect(x: bounds.width - 44, y: 0, width: 44, height: 24)
        slider.frame = NSRect(x: 18, y: 0, width: bounds.width - 18 - 50, height: 24)
    }

    func set(value: Double, gradient: [DocColor]) {
        slider.value = value
        slider.stops = gradient
        if field.currentEditor() == nil {
            field.stringValue = String(Int(value.rounded()))
            shownText = field.stringValue
        }
    }

    /// The text the field last showed; ending an edit without a change does nothing.
    private var shownText = ""

    /// Whole numbers in the channel's range; anything else is refused and the
    /// field shows the current value again.
    public nonisolated static func parse(_ text: String, range: ClosedRange<Double>) -> Double? {
        let t = text.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty, t.allSatisfy(\.isASCII), let v = Int(t), range.contains(Double(v)) else { return nil }
        return Double(v)
    }

    @objc private func entered(_ sender: NSTextField) {
        if sender.stringValue == shownText {
            // No change.
        } else if let v = Self.parse(sender.stringValue, range: channel.range) {
            onChange?(v)
        } else {
            NSSound.beep()
        }
        sender.stringValue = String(Int(slider.value.rounded()))
        shownText = sender.stringValue
        sender.window?.makeFirstResponder(nil)
    }
}

/// A horizontal slider whose track shows the channel's colors and whose thumb
/// is a ring. Drag or click sets the value; arrow keys step by one.
public final class GradientSlider: NSView {
    public let range: ClosedRange<Double>
    public var value: Double = 0 { didSet { if value != oldValue { needsDisplay = true } } }
    public var stops: [DocColor] = [] { didSet { if stops != oldValue { needsDisplay = true } } }
    var checkered = false
    var onChange: ((Double) -> Void)?

    init(range: ClosedRange<Double>) {
        self.range = range
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    var track: NSRect { NSRect(x: 5, y: 8, width: max(bounds.width - 10, 1), height: 8) }

    public override func draw(_ dirtyRect: NSRect) {
        let t = track
        if checkered { ColorDrawing.swatch(DocColor(r: 0, g: 0, b: 0, a: 0), in: t) }
        if stops.count >= 2, let g = NSGradient(colors: stops.map(\.nsColor)) { g.draw(in: t, angle: 0) }
        Theme.panelLine.setStroke()
        NSBezierPath(rect: t.insetBy(dx: -0.5, dy: -0.5)).stroke()
        let f = (value - range.lowerBound) / (range.upperBound - range.lowerBound)
        let x = t.minX + CGFloat(min(max(f, 0), 1)) * t.width
        let ring = NSBezierPath(ovalIn: NSRect(x: x - 5, y: bounds.midY - 5, width: 10, height: 10))
        ring.lineWidth = 2
        NSColor.white.setStroke()
        ring.stroke()
        let inner = NSBezierPath(ovalIn: NSRect(x: x - 6.5, y: bounds.midY - 6.5, width: 13, height: 13))
        inner.lineWidth = 1
        Theme.ink.setStroke()
        inner.stroke()
    }

    func value(at x: CGFloat) -> Double {
        let f = Double(min(max((x - track.minX) / track.width, 0), 1))
        return (range.lowerBound + f * (range.upperBound - range.lowerBound)).rounded()
    }

    private func drag(_ event: NSEvent) {
        let v = value(at: convert(event.locationInWindow, from: nil).x)
        if v != value { onChange?(v) }
    }

    public override func mouseDown(with event: NSEvent) { drag(event) }
    public override func mouseDragged(with event: NSEvent) { drag(event) }

    public override func accessibilityValue() -> Any? { Int(value.rounded()) }
    public override func accessibilityPerformIncrement() -> Bool {
        onChange?(min(value + 1, range.upperBound))
        return true
    }
    public override func accessibilityPerformDecrement() -> Bool {
        onChange?(max(value - 1, range.lowerBound))
        return true
    }

    // M59: Keyboard navigation reaches the slider; the arrow keys step by one.
    public override var acceptsFirstResponder: Bool { KeyboardAccess.enabled }
    public override var canBecomeKeyView: Bool { KeyboardAccess.enabled }
    public override var focusRingMaskBounds: NSRect { track.insetBy(dx: -4, dy: -6) }
    public override func drawFocusRingMask() { track.insetBy(dx: -4, dy: -6).fill() }
    public override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 123, 125: _ = accessibilityPerformDecrement()
        case 124, 126: _ = accessibilityPerformIncrement()
        default: super.keyDown(with: event)
        }
    }
}

// MARK: Palette grid

/// The palette swatches: 8 columns of 24 x 24 pt targets with 2 pt gaps,
/// scrolling when they do not fit. The swatch equal to the active color has a
/// 2 pt outline and a check mark.
@MainActor
public final class PaletteView: NSView {
    public var colors: [DocColor] = [] { didSet { if colors != oldValue { rebuild() } } }
    public var chosen: Int? { didSet { if chosen != oldValue { cells.forEach { $0.needsDisplay = true } } } }
    var addMode = false { didSet { if addMode != oldValue { needsDisplay = true } } }
    /// (index, other slot).
    var onPick: ((Int, Bool) -> Void)?
    public private(set) var cells: [PaletteCell] = []
    let scroll = NSScrollView()
    private let grid = FlippedView()
    static let cell: CGFloat = 24
    static let gap: CGFloat = 2
    static let columns = 8

    public static var gridWidth: CGFloat { CGFloat(columns) * cell + CGFloat(columns - 1) * gap }

    public static func height(rows: Int) -> CGFloat { CGFloat(rows) * cell + CGFloat(max(rows - 1, 0)) * gap }

    public override init(frame: NSRect) {
        super.init(frame: frame)
        scroll.documentView = grid
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        addSubview(scroll)
        setAccessibilityElement(false)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    private func rebuild() {
        if cells.count != colors.count {
            cells.forEach { $0.removeFromSuperview() }
            cells = colors.indices.map { i in
                let c = PaletteCell(index: i, owner: self)
                grid.addSubview(c)
                return c
            }
            needsLayout = true
        }
        for c in cells { c.needsDisplay = true; c.updateAccessibility() }
    }

    public override func layout() {
        super.layout()
        scroll.frame = bounds
        let x0 = ((bounds.width - Self.gridWidth) / 2).rounded()
        for (i, c) in cells.enumerated() {
            c.frame = NSRect(
                x: x0 + CGFloat(i % Self.columns) * (Self.cell + Self.gap),
                y: CGFloat(i / Self.columns) * (Self.cell + Self.gap), width: Self.cell, height: Self.cell)
        }
        let rows = (cells.count + Self.columns - 1) / Self.columns
        grid.frame = NSRect(x: 0, y: 0, width: scroll.contentSize.width,
                            height: max(Self.height(rows: rows), scroll.contentSize.height))
    }

    public override func draw(_ dirtyRect: NSRect) {
        if addMode {
            // Add mode: an accent outline round the grid says "click a slot".
            Theme.accent.setStroke()
            let p = NSBezierPath(rect: bounds.insetBy(dx: 1, dy: 1))
            p.lineWidth = 2
            p.setLineDash([4, 3], count: 2, phase: 0)
            p.stroke()
        }
    }
}

public final class PaletteCell: NSView {
    public let index: Int
    unowned let owner: PaletteView

    init(index: Int, owner: PaletteView) {
        self.index = index
        self.owner = owner
        super.init(frame: NSRect(x: 0, y: 0, width: 24, height: 24))
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        updateAccessibility()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    var color: DocColor { index < owner.colors.count ? owner.colors[index] : Palette.padding }
    var isChosen: Bool { owner.chosen == index }

    func updateAccessibility() {
        setAccessibilityLabel(String(format: L10n.tr("Palette color %d, %@", "VoiceOver name of a palette swatch: number, hex"),
                                     index + 1, color.hex))
        setAccessibilitySelected(isChosen)
        toolTip = String(format: L10n.tr("%@: click for the active color, right-click or Shift-click for the other",
                                         "Tooltip of a palette swatch; %@ is its hex color"), color.hex)
    }

    public override func draw(_ dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 2, dy: 2)
        ColorDrawing.swatch(color, in: r)
        Theme.line.setStroke()
        NSBezierPath(rect: r.insetBy(dx: 0.5, dy: 0.5)).stroke()
        guard isChosen else { return }
        Theme.accent.setStroke()
        let o = NSBezierPath(rect: bounds.insetBy(dx: 1, dy: 1))
        o.lineWidth = 2
        o.stroke()
        // The check mark, white over a dark edge so it shows on any color.
        let check = NSBezierPath()
        check.move(to: NSPoint(x: 7, y: 12.5))
        check.line(to: NSPoint(x: 10.5, y: 16))
        check.line(to: NSPoint(x: 17, y: 8.5))
        check.lineCapStyle = .round
        check.lineJoinStyle = .round
        check.lineWidth = 3.5
        NSColor.black.withAlphaComponent(0.7).setStroke()
        check.stroke()
        check.lineWidth = 1.75
        NSColor.white.setStroke()
        check.stroke()
    }

    public override func mouseDown(with event: NSEvent) {
        let other = !event.modifierFlags.intersection([.shift, .control]).isEmpty
        owner.onPick?(index, other)
    }

    public override func rightMouseDown(with event: NSEvent) { owner.onPick?(index, true) }

    public override func accessibilityPerformPress() -> Bool {
        owner.onPick?(index, false)
        return true
    }

    // M59: Keyboard navigation reaches each swatch. Space or Return sets the
    // active color, Shift-Space the other one; the arrow keys move in the grid.
    public override var acceptsFirstResponder: Bool { KeyboardAccess.enabled }
    public override var canBecomeKeyView: Bool { KeyboardAccess.enabled }
    public override var focusRingMaskBounds: NSRect { bounds }
    public override func drawFocusRingMask() { bounds.fill() }
    public override func keyDown(with event: NSEvent) {
        if KeyboardAccess.isPress(event) { return owner.onPick?(index, event.modifierFlags.contains(.shift)) ?? () }
        let step: Int? = switch event.keyCode {
        case 123: -1
        case 124: 1
        case 125: PaletteView.columns
        case 126: -PaletteView.columns
        default: nil
        }
        if let step, owner.cells.indices.contains(index + step) {
            window?.makeFirstResponder(owner.cells[index + step])
            return
        }
        super.keyDown(with: event)
    }
}

// MARK: More and Less sizing

extension PanelLayout {
    /// The expanded Colors panel's most height (SPEC: at most 470 pt).
    public static let colorsExpandedHeight: CGFloat = 470

    /// Colors and Tools frames for More (expanded) or Less. Colors keeps its
    /// bottom edge and grows up to 470 pt or the room left under Tools at its
    /// 94 pt minimum; Tools, when it sits above Colors, shortens to leave a
    /// 12 pt gap. Less goes back to 246 pt and gives Tools `restoreTools`.
    public static func colorsFrames(colors c: CGRect, tools t: CGRect?, expanded: Bool, bounds: CGRect,
                                    restoreTools: CGRect? = nil) -> (colors: CGRect, tools: CGRect?) {
        func stacked(_ t: CGRect) -> Bool { t.maxX > c.minX && t.minX < c.maxX && t.midY > c.midY }
        if !expanded {
            let colors = CGRect(x: c.minX, y: c.minY, width: c.width, height: PanelKind.colors.defaultSize.height)
            return (colors, restoreTools ?? t)
        }
        var limit = bounds.maxY - clearance
        if let t, stacked(t) { limit = min(limit, t.maxY - PanelKind.tools.minimumSize.height - clearance) }
        let h = max(PanelKind.colors.minimumSize.height, min(colorsExpandedHeight, limit - c.minY))
        let colors = CGRect(x: c.minX, y: c.minY, width: c.width, height: h)
        guard let t, stacked(t), t.minY < colors.maxY + clearance else { return (colors, t) }
        let height = max(PanelKind.tools.minimumSize.height, t.maxY - (colors.maxY + clearance))
        return (colors, CGRect(x: t.minX, y: t.maxY - height, width: t.width, height: height))
    }
}

extension PanelManager {
    /// Applies More or Less to the Colors panel (and Tools above it). Returns
    /// the Tools frame from before an expand, for the matching Less.
    @discardableResult
    public func setColorsExpanded(_ on: Bool, restoreTools: CGRect? = nil) -> CGRect? {
        let before = frame(.tools)
        let bounds = stage() ?? screens().first { $0.intersects(frame(.colors)) } ?? frame(.colors)
        let tools: CGRect? = isShown(.tools) ? before : nil
        let r = PanelLayout.colorsFrames(colors: frame(.colors), tools: tools, expanded: on, bounds: bounds,
                                         restoreTools: on ? nil : restoreTools)
        var frames: [PanelKind: CGRect] = [.colors: r.colors]
        if let t = r.tools, isShown(.tools) { frames[.tools] = t }
        setFrames(frames)
        save()
        onChange?()
        return on ? before : nil
    }
}
