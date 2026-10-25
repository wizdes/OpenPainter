// The Text tool's part of the options row (M46), in paint.net's order
// (https://www.getpaint.net/doc/latest/TextTool.html): font, size, bold,
// italic, underline, strikethrough, alignment, antialiasing, blend mode, and
// Finish. Sizes follow docs/design/SPEC.md: 28 pt controls, 12 pt labels 8 pt
// from their control, 16 pt between groups.
import AppKit

@MainActor
public final class TextOptionsControls: NSObject, NSComboBoxDelegate {
    let tools: ToolState
    public let font = Labels.popup([], width: 170)
    public let size = NSComboBox()
    public private(set) var bold: ToggleButton!
    public private(set) var italic: ToggleButton!
    public private(set) var underline: ToggleButton!
    public private(set) var strikethrough: ToggleButton!
    public private(set) var alignments: [TextStyleSpec.Alignment: ToggleButton] = [:]
    public let finish = NSButton(title: "Finish", target: nil, action: nil)
    /// The window's Text tool commits its text.
    public var onFinish: (() -> Void)?
    private var families: [String] = []

    init(tools: ToolState) {
        self.tools = tools
        super.init()
        func toggle(_ symbol: String, _ label: String, _ key: String) -> ToggleButton {
            let b = ToggleButton(symbol: symbol, label: label, tooltip: key.isEmpty ? label : "\(label) (\(key))",
                                 pointSize: 14, target: self, action: #selector(styleClicked(_:)))
            return b
        }
        bold = toggle("bold", "Bold", "")
        italic = toggle("italic", "Italic", "")
        underline = toggle("underline", "Underline", "")
        strikethrough = toggle("strikethrough", "Strikethrough", "")
        for (a, symbol) in [(TextStyleSpec.Alignment.left, "text.alignleft"), (.center, "text.aligncenter"),
                            (.right, "text.alignright")] {
            let b = ToggleButton(symbol: symbol, label: "Align \(a.label)", pointSize: 14, target: self,
                                 action: #selector(alignClicked(_:)))
            alignments[a] = b
        }
        font.target = self
        font.action = #selector(fontPicked(_:))
        font.setAccessibilityLabel("Font")
        size.translatesAutoresizingMaskIntoConstraints = false
        size.widthAnchor.constraint(equalToConstant: 64).isActive = true
        size.heightAnchor.constraint(equalToConstant: 28).isActive = true
        size.font = Theme.font(12)
        size.addItems(withObjectValues: TextToolSettings.sizes.map { OptionsBarView.number($0) })
        size.numberOfVisibleItems = 12
        size.completes = false
        size.target = self
        size.action = #selector(sizeChanged(_:))
        size.delegate = self
        size.setAccessibilityLabel("Font size")
        finish.bezelStyle = .rounded
        finish.font = Theme.font(12)
        finish.target = self
        finish.action = #selector(finishClicked(_:))
        finish.setAccessibilityLabel("Finish text")
        finish.toolTip = "Finish: commit the text to the layer"
    }

    /// Fills the font menu once, the first time the Text options show.
    func loadFamilies() {
        guard families.isEmpty else { return }
        families = TextLayoutEngine.availableFamilies()
        font.removeAllItems()
        font.addItems(withTitles: families)
    }

    func add(_ add: (NSView...) -> Void, group: () -> Void, antialias: NSButton, blend: NSPopUpButton) {
        loadFamilies()
        add(Labels.label("Font"), font, size, Labels.label("pt"))
        group()
        add(bold, italic, underline, strikethrough)
        group()
        add(alignments[.left]!, alignments[.center]!, alignments[.right]!)
        group()
        add(Labels.label("Antialiasing"), antialias)
        group()
        add(Labels.label("Blend mode"), blend)
        group()
        add(finish)
    }

    func sync() {
        let t = tools.settings.text
        if font.titleOfSelectedItem != t.family {
            if font.indexOfItem(withTitle: t.family) < 0, !families.isEmpty {
                // A family that is not installed still shows, so the user sees what was asked for.
                font.addItem(withTitle: t.family)
            }
            font.selectItem(withTitle: t.family)
        }
        if size.currentEditor() == nil { size.stringValue = OptionsBarView.number(t.size) }
        bold.isOn = t.bold
        italic.isOn = t.italic
        underline.isOn = t.underline
        strikethrough.isOn = t.strikethrough
        for (a, b) in alignments { b.isOn = a == t.alignment }
    }

    @objc private func fontPicked(_ sender: NSPopUpButton) {
        guard let title = sender.titleOfSelectedItem else { return }
        tools.settings.text.family = title
    }

    @objc private func sizeChanged(_ sender: NSComboBox) {
        let v = Double(sender.stringValue.trimmingCharacters(in: .whitespaces)) ?? .nan
        if v.isFinite {
            tools.settings.text.size = min(max(v, TextStyleSpec.sizeRange.lowerBound), TextStyleSpec.sizeRange.upperBound)
        }
        sync()  // shows the clamped value, or the old one after bad input
    }

    public func comboBoxSelectionDidChange(_ notification: Notification) {
        let i = size.indexOfSelectedItem
        guard i >= 0, i < TextToolSettings.sizes.count else { return }
        tools.settings.text.size = TextToolSettings.sizes[i]
    }

    @objc private func styleClicked(_ sender: ToggleButton) {
        var t = tools.settings.text
        if sender === bold { t.bold.toggle() }
        if sender === italic { t.italic.toggle() }
        if sender === underline { t.underline.toggle() }
        if sender === strikethrough { t.strikethrough.toggle() }
        tools.settings.text = t
        sync()
    }

    @objc private func alignClicked(_ sender: ToggleButton) {
        if let a = alignments.first(where: { $0.value === sender })?.key { tools.settings.text.alignment = a }
        sync()
    }

    @objc private func finishClicked(_ sender: Any?) { onFinish?() }
}
