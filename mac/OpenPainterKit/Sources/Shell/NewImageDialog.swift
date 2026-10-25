// File > New (M19), as paint.net 5.1 describes it
// (https://www.getpaint.net/doc/latest/FileMenu.html, New): the estimated
// memory size at the top, Maintain aspect ratio, the pixel size, the
// resolution and the print size (pixel size / resolution = print size), and
// the background: White, Secondary color or Transparent. paint.net's default
// is 800 x 600 pixels; on the Mac the resolution starts at 96 pixels/inch,
// paint.net's value at 100% system scaling.
//
// The layout and controls follow the M23 Resize and Canvas Size dialogs
// (ImageSizeDialogs.swift), which share ImageSizeModel with this one.
import AppKit

/// The New Image background (paint.net's choices).
public enum NewImageBackground: String, CaseIterable, Sendable {
    case white, secondary, transparent

    public var label: String {
        switch self {
        case .white: "White"
        case .secondary: "Secondary color"
        case .transparent: "Transparent"
        }
    }
}

/// What OK in the New Image dialog makes.
public struct NewImageSettings: Equatable, Sendable {
    public var width: Int
    public var height: Int
    /// Pixels per inch.
    public var dpi: Double
    public var background: NewImageBackground
    /// The secondary color when the dialog opened (used for `.secondary`).
    public var secondary: DocColor

    public init(width: Int = 800, height: Int = 600, dpi: Double = 96, background: NewImageBackground = .white,
                secondary: DocColor = DocColor(r: 255, g: 255, b: 255)) {
        self.width = width
        self.height = height
        self.dpi = dpi
        self.background = background
        self.secondary = secondary
    }

    /// The fill as straight RGBA8.
    public var rgba: [UInt8] {
        switch background {
        case .white: [255, 255, 255, 255]
        case .secondary: [secondary.r, secondary.g, secondary.b, secondary.a]
        case .transparent: [0, 0, 0, 0]
        }
    }
}

/// The New Image dialog. OK calls `onFinish` with the settings, Cancel with nil.
@MainActor
public final class NewImageController: NSWindowController, NSTextFieldDelegate {
    public private(set) var model: ImageSizeModel
    public var background: NewImageBackground { didSet { show() } }
    public let secondary: DocColor
    let content = FilledView(fill: Theme.panel)

    public let keepAspect = NSButton(checkboxWithTitle: "Maintain aspect ratio", target: nil, action: nil)
    public let widthField = Labels.field("", width: 80)
    public let heightField = Labels.field("", width: 80)
    public let resolutionField = Labels.field("", width: 80)
    public let unitPopup = Labels.popup(PrintUnit.allCases.map(\.resolutionLabel), width: 150)
    public let printWidthField = Labels.field("", width: 80)
    public let printHeightField = Labels.field("", width: 80)
    let printWidthUnit = Labels.label("inches")
    let printHeightUnit = Labels.label("inches")
    public let backgroundRadios: [NewImageBackground: NSButton]
    let swatch = FilledView(fill: .white)
    /// "Image size: 1.8 MB (800 × 600 px)", updated on every change.
    public let resultLabel = Labels.label("", size: 12, color: Theme.ink, weight: .semibold)
    public private(set) var okButton: NSButton!
    public private(set) var cancelButton: NSButton!
    public var onFinish: ((NewImageSettings?) -> Void)?

    public init(settings: NewImageSettings = NewImageSettings()) {
        var m = ImageSizeModel(width: settings.width, height: settings.height, dpi: settings.dpi)
        m.byPercent = false
        m.keepAspect = false
        model = m
        background = settings.background
        secondary = settings.secondary
        var radios: [NewImageBackground: NSButton] = [:]
        for b in NewImageBackground.allCases {
            radios[b] = NSButton(radioButtonWithTitle: b.label, target: nil, action: nil)
        }
        backgroundRadios = radios
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: 470), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = "New Image"
        panel.isReleasedWhenClosed = false
        super.init(window: panel)
        panel.contentView = content
        build()
        show()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// The settings OK would send now.
    public var settings: NewImageSettings {
        NewImageSettings(width: model.width, height: model.height, dpi: model.dpi, background: background,
                         secondary: secondary)
    }

    // y grows down (FilledView is flipped).
    private func place(_ v: NSView, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat = 24) {
        v.translatesAutoresizingMaskIntoConstraints = true
        v.frame = NSRect(x: x, y: y, width: w, height: h)
        content.addSubview(v)
    }

    private func label(_ text: String, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat = 96, right: Bool = true,
                       color: NSColor = Theme.ink) {
        let l = Labels.label(text, color: color)
        l.alignment = right ? .right : .left
        place(l, x, y + 4, w, 16)
    }

    private func build() {
        var y: CGFloat = 16
        place(resultLabel, 16, y, 388, 18)
        y += 30
        keepAspect.font = Theme.font(12)
        place(keepAspect, 16, y, 200)
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
        y += 34
        label("Background", 16, y, 120, right: false, color: Theme.muted)
        y += 24
        for b in NewImageBackground.allCases {
            let r = backgroundRadios[b]!
            r.font = Theme.font(12)
            r.target = self
            r.action = #selector(backgroundChanged(_:))
            place(r, 36, y, b == .secondary ? 128 : 180)
            if b == .secondary {
                swatch.wantsLayer = true
                swatch.layer?.borderWidth = 1
                swatch.layer?.borderColor = Theme.line.cgColor
                swatch.layer?.cornerRadius = 3
                place(swatch, 168, y + 3, 18, 18)
                swatch.setAccessibilityLabel("Secondary color")
            }
            y += 26
        }
        y += 14
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        okButton = NSButton(title: "OK", target: self, action: #selector(ok(_:)))
        okButton.keyEquivalent = "\r"
        place(cancelButton, 240, y, 80, 28)
        place(okButton, 324, y, 80, 28)
        window?.setContentSize(NSSize(width: 420, height: y + 28 + 16))

        keepAspect.target = self
        keepAspect.action = #selector(controlChanged(_:))
        unitPopup.target = self
        unitPopup.action = #selector(controlChanged(_:))
        for (f, name) in [(widthField, "Width in pixels"), (heightField, "Height in pixels"),
                          (resolutionField, "Resolution"), (printWidthField, "Print width"),
                          (printHeightField, "Print height")] {
            f.delegate = self
            f.target = self
            f.action = #selector(fieldTyped(_:))
            f.alignment = .right
            f.setAccessibilityLabel(name)
        }
    }

    /// Copies the model into the controls (fields being edited keep their text).
    func show() {
        let m = model
        keepAspect.state = m.keepAspect ? .on : .off
        func set(_ f: NSTextField, _ s: String) { if f.currentEditor() == nil { f.stringValue = s } }
        set(widthField, String(m.width))
        set(heightField, String(m.height))
        set(resolutionField, ImageSizeController.text(m.resolution))
        set(printWidthField, ImageSizeController.text(m.printWidth))
        set(printHeightField, ImageSizeController.text(m.printHeight))
        unitPopup.selectItem(at: PrintUnit.allCases.firstIndex(of: m.unit) ?? 0)
        printWidthUnit.stringValue = m.unit.label
        printHeightUnit.stringValue = m.unit.label
        for (b, r) in backgroundRadios { r.state = b == background ? .on : .off }
        swatch.fill = NSColor(srgbRed: CGFloat(secondary.r) / 255, green: CGFloat(secondary.g) / 255,
                              blue: CGFloat(secondary.b) / 255, alpha: 1)
        resultLabel.stringValue = "Image size: \(m.memoryText) (\(m.sizeText))"
    }

    /// Changes the model (also used by tests) and shows it.
    public func update(_ body: (inout ImageSizeModel) -> Void) {
        body(&model)
        model.byPercent = false
        show()
    }

    @objc private func controlChanged(_ sender: Any?) {
        update { m in
            m.keepAspect = keepAspect.state == .on
            m.unit = PrintUnit.allCases[max(unitPopup.indexOfSelectedItem, 0)]
        }
    }

    @objc private func backgroundChanged(_ sender: NSButton) {
        if let b = backgroundRadios.first(where: { $0.value === sender })?.key { background = b }
    }

    private func apply(_ field: NSTextField) {
        let v = Double(field.stringValue.trimmingCharacters(in: .whitespaces)) ?? .nan
        guard v.isFinite, v > 0 else { return }
        update { m in
            switch field {
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

    @objc private func fieldTyped(_ sender: NSTextField) {
        apply(sender)
        window?.makeFirstResponder(nil)
        show()
    }

    /// Shows the dialog centered on the screen.
    public func present() {
        show()
        window?.center()
        window?.makeKeyAndOrderFront(nil)
        window?.makeFirstResponder(widthField)
    }

    @objc public func ok(_ sender: Any?) {
        let s = settings
        window?.orderOut(nil)
        onFinish?(s)
        onFinish = nil
    }

    @objc public func cancel(_ sender: Any?) {
        window?.orderOut(nil)
        onFinish?(nil)
        onFinish = nil
    }
}
