// Image > Resize and Image > Canvas Size (M23), as paint.net 5.1 describes
// them (https://www.getpaint.net/doc/latest/ImageMenu.html): by percentage or
// by absolute size, Maintain aspect ratio, pixel size and print size with a
// resolution (pixel size / resolution = print size), the resampling mode
// (Resize), the anchor of 9 and the fill (Canvas Size, fill added in 5.1.1).
// Both show the resulting size before OK and send the existing engine command
// (`image.resize`, `image.canvas_size`) once: one history step.
//
// The resolution only converts between pixel and print size here. The engine
// has no command that stores a new resolution in the document yet.
import AppKit

/// Inches or centimeters for the print size.
public enum PrintUnit: String, CaseIterable, Sendable {
    case inches, centimeters
    public var label: String { self == .inches ? "inches" : "centimeters" }
    public var resolutionLabel: String { self == .inches ? "pixels/inch" : "pixels/centimeter" }
    /// Units per inch.
    var perInch: Double { self == .inches ? 1 : 2.54 }
}

/// The pixel size, percentage and print size of a size dialog, kept in step.
public struct ImageSizeModel: Equatable, Sendable {
    public let originalWidth: Int
    public let originalHeight: Int
    public var byPercent = true
    public var percent: Double = 100
    public private(set) var width: Int
    public private(set) var height: Int
    public var keepAspect = true
    public var unit = PrintUnit.inches
    /// Pixels per inch.
    public private(set) var dpi: Double

    public init(width: Int, height: Int, dpi: Double) {
        originalWidth = width
        originalHeight = height
        self.width = width
        self.height = height
        self.dpi = dpi > 0 ? dpi : 96
    }

    static let maxSide = 65_535

    static func clamp(_ v: Double) -> Int { Int(min(max(v.rounded(), 1), Double(maxSide))) }

    public mutating func setPercent(_ p: Double) {
        guard p.isFinite, p > 0 else { return }
        byPercent = true
        percent = p
        width = Self.clamp(Double(originalWidth) * p / 100)
        height = Self.clamp(Double(originalHeight) * p / 100)
    }

    public mutating func setWidth(_ w: Int) {
        byPercent = false
        width = Self.clamp(Double(w))
        if keepAspect { height = Self.clamp(Double(width) * Double(originalHeight) / Double(originalWidth)) }
    }

    public mutating func setHeight(_ h: Int) {
        byPercent = false
        height = Self.clamp(Double(h))
        if keepAspect { width = Self.clamp(Double(height) * Double(originalWidth) / Double(originalHeight)) }
    }

    /// Pixels per print unit.
    public var resolution: Double { dpi / unit.perInch }
    public var printWidth: Double { Double(width) / resolution }
    public var printHeight: Double { Double(height) / resolution }

    public mutating func setPrintWidth(_ v: Double) {
        guard v.isFinite, v > 0 else { return }
        setWidth(Self.clamp(v * resolution))
    }

    public mutating func setPrintHeight(_ v: Double) {
        guard v.isFinite, v > 0 else { return }
        setHeight(Self.clamp(v * resolution))
    }

    /// A new resolution keeps the print size, so the pixel size changes
    /// (pixel size = print size × resolution).
    public mutating func setResolution(_ perUnit: Double) {
        guard perUnit.isFinite, perUnit > 0 else { return }
        let (pw, ph) = (printWidth, printHeight)
        dpi = perUnit * unit.perInch
        byPercent = false
        width = Self.clamp(pw * resolution)
        height = Self.clamp(ph * resolution)
    }

    /// The uncompressed size in memory, as paint.net shows it ("New size").
    public var memoryText: String {
        ByteCountFormatter.string(fromByteCount: Int64(width) * Int64(height) * 4, countStyle: .memory)
    }

    public var sizeText: String { "\(width) × \(height) px" }
}

/// The engine's resampling modes, with paint.net's names.
public enum ResizeResampling: String, CaseIterable, Sendable {
    case bicubic, bicubic_smooth, bilinear, bilinear_low_quality, adaptive_sharp, lanczos, fant,
        nearest_neighbor, super_sampling, best_quality

    public var label: String {
        switch self {
        case .bicubic: "Bicubic"
        case .bicubic_smooth: "Bicubic (Smooth)"
        case .bilinear: "Bilinear"
        case .bilinear_low_quality: "Bilinear (Low Quality)"
        case .adaptive_sharp: "Adaptive (Sharp)"
        case .lanczos: "Lanczos"
        case .fant: "Fant"
        case .nearest_neighbor: "Nearest Neighbor"
        case .super_sampling: "Super Sampling"
        case .best_quality: "Best Quality"
        }
    }
}

/// The nine Canvas Size anchors, in reading order.
public enum CanvasAnchor: String, CaseIterable, Sendable {
    case top_left, top, top_right, left, center, right, bottom_left, bottom, bottom_right

    var symbol: String {
        switch self {
        case .top_left: "arrow.up.left"
        case .top: "arrow.up"
        case .top_right: "arrow.up.right"
        case .left: "arrow.left"
        case .center: "circle.fill"
        case .right: "arrow.right"
        case .bottom_left: "arrow.down.left"
        case .bottom: "arrow.down"
        case .bottom_right: "arrow.down.right"
        }
    }

    public var label: String { rawValue.replacingOccurrences(of: "_", with: " ").capitalized }
}

public enum CanvasFill: String, CaseIterable, Sendable {
    case transparent, primary, secondary, white, black
    public var label: String {
        switch self {
        case .transparent: "Transparent"
        case .primary: "Primary Color"
        case .secondary: "Secondary Color"
        case .white: "White"
        case .black: "Black"
        }
    }
}

/// The controls both dialogs share: percentage, absolute size, aspect, print size.
@MainActor
public class ImageSizeController: NSWindowController, NSTextFieldDelegate {
    public let editor: Editor
    public private(set) var model: ImageSizeModel
    let content = FilledView(fill: Theme.panel)

    public let percentRadio = NSButton(radioButtonWithTitle: "By percentage:", target: nil, action: nil)
    public let absoluteRadio = NSButton(radioButtonWithTitle: "By absolute size", target: nil, action: nil)
    public let percentField = Labels.field("100", width: 64)
    public let keepAspect = NSButton(checkboxWithTitle: "Maintain aspect ratio", target: nil, action: nil)
    public let widthField = Labels.field("", width: 80)
    public let heightField = Labels.field("", width: 80)
    public let resolutionField = Labels.field("", width: 80)
    public let unitPopup = Labels.popup(PrintUnit.allCases.map(\.resolutionLabel), width: 150)
    public let printWidthField = Labels.field("", width: 80)
    public let printHeightField = Labels.field("", width: 80)
    let printWidthUnit = Labels.label("inches")
    let printHeightUnit = Labels.label("inches")
    /// "New size: 1.2 MB (W × H px)", updated on every change.
    public let resultLabel = Labels.label("", size: 12, color: Theme.ink, weight: .semibold)
    public private(set) var okButton: NSButton!
    public private(set) var cancelButton: NSButton!
    /// Called when the sheet closes, with true for OK.
    var onClose: ((Bool) -> Void)?
    /// The ticket of the command OK sent.
    public private(set) var sentTicket: UInt64?

    static var openDialogs: [ObjectIdentifier: ImageSizeController] = [:]
    public static func current(for editor: Editor) -> ImageSizeController? { openDialogs[ObjectIdentifier(editor)] }

    init(editor: Editor, dpi: Double, title: String, height: CGFloat) {
        self.editor = editor
        model = ImageSizeModel(width: editor.info.width, height: editor.info.height, dpi: dpi)
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: height), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = title
        super.init(window: panel)
        panel.contentView = content
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    // y grows down (FilledView is flipped).
    func place(_ v: NSView, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat = 24) {
        v.translatesAutoresizingMaskIntoConstraints = true
        v.frame = NSRect(x: x, y: y, width: w, height: h)
        content.addSubview(v)
    }

    func label(_ text: String, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat = 96, right: Bool = true,
               color: NSColor = Theme.ink) {
        let l = Labels.label(text, color: color)
        l.alignment = right ? .right : .left
        place(l, x, y + 4, w, 16)
    }

    /// Lays out the shared size controls from `top`; returns the next y.
    func buildSizeControls(top: CGFloat) -> CGFloat {
        var y = top
        place(resultLabel, 16, y, 388, 18)
        y += 30
        percentRadio.font = Theme.font(12)
        place(percentRadio, 16, y, 130)
        place(percentField, 150, y, 64)
        label("%", 218, y, 20, right: false)
        y += 32
        absoluteRadio.font = Theme.font(12)
        place(absoluteRadio, 16, y, 200)
        y += 30
        keepAspect.font = Theme.font(12)
        place(keepAspect, 36, y, 200)
        y += 32
        label("Pixel size", 16, y, 120, right: false, color: Theme.muted)
        y += 24
        label("Width:", 16, y)
        place(widthField, 120, y, 80)
        label("pixels", 206, y, 60, right: false)
        y += 30
        label("Height:", 16, y)
        place(heightField, 120, y, 80)
        label("pixels", 206, y, 60, right: false)
        y += 34
        label("Print size", 16, y, 120, right: false, color: Theme.muted)
        y += 24
        label("Resolution:", 16, y)
        place(resolutionField, 120, y, 80)
        place(unitPopup, 206, y - 2, 160, 28)
        y += 32
        label("Width:", 16, y)
        place(printWidthField, 120, y, 80)
        place(printWidthUnit, 206, y + 4, 100, 16)
        y += 30
        label("Height:", 16, y)
        place(printHeightField, 120, y, 80)
        place(printHeightUnit, 206, y + 4, 100, 16)
        y += 36
        for b in [percentRadio, absoluteRadio, keepAspect] {
            b.target = self
            b.action = #selector(controlChanged(_:))
        }
        unitPopup.target = self
        unitPopup.action = #selector(controlChanged(_:))
        unitPopup.setAccessibilityLabel("Resolution unit")  // M59: VoiceOver name
        for (f, name) in [(percentField, "Percentage"), (widthField, "Width in pixels"), (heightField, "Height in pixels"),
                          (resolutionField, "Resolution"), (printWidthField, "Print width"),
                          (printHeightField, "Print height")] {
            f.delegate = self
            f.target = self
            f.action = #selector(fieldTyped(_:))
            f.alignment = .right
            f.setAccessibilityLabel(name)
        }
        return y
    }

    func buildButtons(y: CGFloat) {
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        okButton = NSButton(title: "OK", target: self, action: #selector(ok(_:)))
        okButton.keyEquivalent = "\r"
        place(cancelButton, 240, y, 80, 28)
        place(okButton, 324, y, 80, 28)
    }

    static func text(_ v: Double, digits: Int = 2) -> String {
        if v.rounded() == v { return String(Int(v)) }
        return String(format: "%.\(digits)f", v)
    }

    /// Copies the model into the controls (fields being edited keep their text).
    func show() {
        let m = model
        percentRadio.state = m.byPercent ? .on : .off
        absoluteRadio.state = m.byPercent ? .off : .on
        keepAspect.state = m.keepAspect ? .on : .off
        func set(_ f: NSTextField, _ s: String) { if f.currentEditor() == nil { f.stringValue = s } }
        set(percentField, Self.text(m.percent))
        set(widthField, String(m.width))
        set(heightField, String(m.height))
        set(resolutionField, Self.text(m.resolution))
        set(printWidthField, Self.text(m.printWidth))
        set(printHeightField, Self.text(m.printHeight))
        unitPopup.selectItem(at: PrintUnit.allCases.firstIndex(of: m.unit) ?? 0)
        printWidthUnit.stringValue = m.unit.label
        printHeightUnit.stringValue = m.unit.label
        percentField.isEnabled = m.byPercent
        for f in [widthField, heightField, resolutionField, printWidthField, printHeightField] { f.isEnabled = !m.byPercent }
        keepAspect.isEnabled = !m.byPercent
        resultLabel.stringValue = "New size: \(m.memoryText) (\(m.sizeText))"
    }

    /// Changes the model (also used by tests) and shows it.
    public func update(_ body: (inout ImageSizeModel) -> Void) {
        body(&model)
        show()
    }

    @objc func controlChanged(_ sender: Any?) {
        update { m in
            if (sender as? NSButton) === percentRadio { m.setPercent(m.percent) }
            if (sender as? NSButton) === absoluteRadio { m.byPercent = false }
            m.keepAspect = keepAspect.state == .on
            m.unit = PrintUnit.allCases[max(unitPopup.indexOfSelectedItem, 0)]
        }
    }

    func apply(_ field: NSTextField) {
        let v = Double(field.stringValue.trimmingCharacters(in: .whitespaces)) ?? .nan
        guard v.isFinite, v > 0 else { return }
        update { m in
            switch field {
            case percentField: m.setPercent(v)
            case widthField: m.setWidth(Int(v.rounded()))
            case heightField: m.setHeight(Int(v.rounded()))
            case resolutionField: m.setResolution(v)
            case printWidthField: m.setPrintWidth(v)
            case printHeightField: m.setPrintHeight(v)
            default: break
            }
        }
    }

    public func controlTextDidChange(_ note: Notification) {
        guard let f = note.object as? NSTextField else { return }
        apply(f)
    }

    @objc func fieldTyped(_ sender: NSTextField) {
        apply(sender)
        window?.makeFirstResponder(nil)
        show()
    }

    /// The command and parameters OK sends.
    public var command: (String, [String: any Sendable]) { fatalError("subclass") }

    public func present(on window: NSWindow) {
        Self.openDialogs[ObjectIdentifier(editor)] = self
        show()
        window.beginSheet(self.window!) { _ in }
    }

    @objc public func ok(_ sender: Any?) {
        let (id, params) = command
        close(true)
        sentTicket = editor.run(id, params)
    }

    @objc public func cancel(_ sender: Any?) { close(false) }

    func close(_ ok: Bool) {
        Self.openDialogs[ObjectIdentifier(editor)] = nil
        if let sheet = window, let parent = sheet.sheetParent { parent.endSheet(sheet) }
        window?.orderOut(nil)
        onClose?(ok)
    }
}

/// Image > Resize.
@MainActor
public final class ResizeImageController: ImageSizeController {
    public let resamplingPopup = Labels.popup(ResizeResampling.allCases.map(\.label), width: 200)
    public let gammaBox = NSButton(checkboxWithTitle: "Gamma correction", target: nil, action: nil)
    public var resampling = ResizeResampling.bicubic { didSet { show() } }
    public var gamma = true { didSet { show() } }

    public init(editor: Editor, dpi: Double) {
        super.init(editor: editor, dpi: dpi, title: "Resize Image", height: 470)
        var y = buildSizeControls(top: 16)
        label("Resampling:", 16, y)
        place(resamplingPopup, 118, y - 2, 200, 28)
        resamplingPopup.setAccessibilityLabel("Resampling")
        resamplingPopup.target = self
        resamplingPopup.action = #selector(resamplingChanged(_:))
        y += 32
        gammaBox.font = Theme.font(12)
        gammaBox.target = self
        gammaBox.action = #selector(resamplingChanged(_:))
        place(gammaBox, 118, y, 200)
        y += 40
        buildButtons(y: y)
    }

    @objc private func resamplingChanged(_ sender: Any?) {
        resampling = ResizeResampling.allCases[max(resamplingPopup.indexOfSelectedItem, 0)]
        gamma = gammaBox.state == .on
    }

    override func show() {
        super.show()
        resamplingPopup.selectItem(at: ResizeResampling.allCases.firstIndex(of: resampling) ?? 0)
        gammaBox.state = gamma ? .on : .off
    }

    /// By percentage sends `percent` (the engine rounds the same way);
    /// absolute size sends both sides.
    public override var command: (String, [String: any Sendable]) {
        var p: [String: any Sendable] = ["resampling": resampling.rawValue, "gamma": gamma]
        if model.byPercent {
            p["percent"] = model.percent
            p["width"] = 0
            p["height"] = 0
        } else {
            p["width"] = model.width
            p["height"] = model.height
            p["maintain_aspect"] = false
        }
        return ("image.resize", p)
    }
}

/// Image > Canvas Size.
@MainActor
public final class CanvasSizeController: ImageSizeController {
    public private(set) var anchorButtons: [CanvasAnchor: NSButton] = [:]
    public let fillPopup = Labels.popup(CanvasFill.allCases.map(\.label), width: 160)
    public var anchor = CanvasAnchor.center { didSet { show() } }
    public var fill = CanvasFill.transparent { didSet { show() } }
    let tools: ToolState

    public init(editor: Editor, dpi: Double, tools: ToolState) {
        self.tools = tools
        super.init(editor: editor, dpi: dpi, title: "Canvas Size", height: 580)
        var y = buildSizeControls(top: 16)
        label("Anchor", 16, y, 120, right: false, color: Theme.muted)
        y += 24
        for (i, a) in CanvasAnchor.allCases.enumerated() {
            let b = ShellButton(symbol: a.symbol, label: "Anchor \(a.label)", size: 28, pointSize: 13,
                                target: self, action: #selector(anchorClicked(_:)))
            b.tag = i
            place(b, 120 + CGFloat(i % 3) * 32, y + CGFloat(i / 3) * 32, 28, 28)
            anchorButtons[a] = b
        }
        y += 3 * 32 + 8
        label("Fill:", 16, y)
        place(fillPopup, 118, y - 2, 160, 28)
        fillPopup.setAccessibilityLabel("Fill")
        fillPopup.target = self
        fillPopup.action = #selector(fillChanged(_:))
        y += 40
        buildButtons(y: y)
    }

    @objc private func anchorClicked(_ sender: NSButton) {
        anchor = CanvasAnchor.allCases[sender.tag]
    }

    @objc private func fillChanged(_ sender: Any?) {
        fill = CanvasFill.allCases[max(fillPopup.indexOfSelectedItem, 0)]
    }

    override func show() {
        super.show()
        for (a, b) in anchorButtons {
            let on = a == anchor
            b.contentTintColor = on ? Theme.accent : Theme.muted
            b.layer?.borderWidth = on ? 2 : 0
            b.wantsLayer = true
            b.layer?.borderColor = Theme.accent.cgColor
            b.layer?.cornerRadius = 4
            b.setAccessibilityValue(on ? "selected" : nil)
        }
        fillPopup.selectItem(at: CanvasFill.allCases.firstIndex(of: fill) ?? 0)
    }

    public override var command: (String, [String: any Sendable]) {
        ("image.canvas_size", [
            "width": model.width, "height": model.height, "anchor": anchor.rawValue, "fill": fill.rawValue,
            "primary": tools.primary.engineHex, "secondary": tools.secondary.engineHex,
        ])
    }
}
