// The 36 pt tool-options row: the window's command buttons (CommandBar.swift,
// fixed at the leading end), then the tool picker and the current tool's options.
// Paintbrush: width with - and +, hardness, antialiasing, blend mode, opacity,
// pen pressure for size and for opacity. Eraser: the same without blend mode.
// Pencil (always 1 px, hard, aliased in paint.net): blend mode, opacity, pen
// pressure for opacity. Magic
// Wand and Paint Bucket: tolerance (slider and field), tolerance alpha,
// selection or blend mode, sampling, flood mode, antialiasing and Finish
// (M31: the click stays live until Finish). Zoom and Pan:
// none. Color Picker: sample size, layer or image, after click. At narrow sizes the tool options scroll sideways
// instead of wrapping; the command buttons never scroll or clip.
import AppKit

@MainActor
public final class OptionsBarView: FilledView {
    let tools: ToolState
    private let scroll = NSScrollView()
    private let stack = NSStackView()
    public let picker = NSPopUpButton(frame: .zero, pullsDown: false)
    // Brush controls.
    public let widthField = Labels.field("2", width: 52, height: 28)
    public private(set) var minus: ShellButton!
    public private(set) var plus: ShellButton!
    public let antialias = NSButton(checkboxWithTitle: "On", target: nil, action: nil)
    public let blend = Labels.popup(ToolSettings.Blend.allCases.map(\.label))
    public let opacityField = Labels.field("100", width: 52, height: 28)
    public let hardnessField = Labels.field("100", width: 52, height: 28)
    /// Pen pressure toggles (M38).
    public let pressureSize = NSButton(checkboxWithTitle: "Size", target: nil, action: nil)
    public let pressureOpacity = NSButton(checkboxWithTitle: "Opacity", target: nil, action: nil)
    // Flood controls.
    public let toleranceField = Labels.field("50", width: 52, height: 28)
    /// Live tolerance (M31): dragging it re-makes the open fill or selection.
    public let toleranceSlider = NSSlider(value: 50, minValue: 0, maxValue: 100, target: nil, action: nil)
    public let toleranceAlpha = Labels.popup(ToolSettings.ToleranceAlpha.allCases.map(\.label))
    /// Commits the open Magic Wand or Paint Bucket session (Enter does the same).
    public let floodFinish = NSButton(title: "Finish", target: nil, action: nil)
    public var onFloodFinish: (() -> Void)?
    /// True while a Magic Wand or Paint Bucket session is open: Finish is on.
    public var floodSessionOpen = false { didSet { floodFinish.isEnabled = floodSessionOpen } }
    public let selectionMode = Labels.popup(ToolSettings.SelectionMode.allCases.map(\.label))
    public let sampling = Labels.popup(ToolSettings.Sampling.allCases.map(\.label))
    public let floodMode = Labels.popup(ToolSettings.FloodMode.allCases.map(\.label))
    // Color Picker controls (M36).
    public let pickerSize = Labels.popup(PickerSampleSize.allCases.map(\.label))
    public let pickerSampling = Labels.popup(ToolSettings.Sampling.allCases.map(\.label))
    public let pickerAfterClick = Labels.popup(PickerAfterClick.allCases.map(\.label))
    /// Text tool controls (M46).
    public private(set) lazy var textControls = TextOptionsControls(tools: tools)
    /// Views another controller shows after the tool picker in place of the
    /// tool's options (M23: Rectangle Select and the Crop tool); nil for none.
    public var customOptions: [NSView]? {
        didSet {
            shownSet = nil
            sync()
        }
    }
    /// The window's command buttons, fixed before the tool picker; nil for none.
    public var commandBar: NSView? {
        didSet {
            oldValue?.removeFromSuperview()
            if let commandBar {
                commandBar.translatesAutoresizingMaskIntoConstraints = true
                addSubview(commandBar)
            }
            needsLayout = true
        }
    }
    private var shownSet: PaintTool.OptionSet?
    private var shownTool: PaintTool?

    public init(tools: ToolState) {
        self.tools = tools
        super.init(fill: Theme.options, dividers: [.bottom])
        minus = ShellButton(symbol: "minus", label: "Decrease brush width", pointSize: 12, target: self,
                            action: #selector(stepWidth(_:)))
        plus = ShellButton(symbol: "plus", label: "Increase brush width", pointSize: 12, target: self,
                           action: #selector(stepWidth(_:)))
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 12, bottom: 4, right: 12)
        stack.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = stack
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        scroll.verticalScrollElasticity = .none
        addSubview(scroll)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: scroll.contentView.topAnchor),
            stack.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
            stack.heightAnchor.constraint(equalToConstant: Theme.optionsHeight - 1),
            stack.widthAnchor.constraint(greaterThanOrEqualTo: scroll.contentView.widthAnchor),
        ])
        setUpPicker()
        for f in [widthField, opacityField, toleranceField, hardnessField] {
            f.target = self
            f.action = #selector(fieldChanged(_:))
            f.setAccessibilityLabel(
                f === widthField ? "Brush width" : f === opacityField ? "Opacity"
                    : f === hardnessField ? "Hardness" : "Tolerance")
        }
        for b in [pressureSize, pressureOpacity] {
            b.target = self
            b.action = #selector(controlChanged(_:))
            b.font = Theme.font(12)
        }
        pressureSize.setAccessibilityLabel("Pen pressure changes the brush width")
        pressureOpacity.setAccessibilityLabel("Pen pressure changes the opacity")
        antialias.target = self
        antialias.action = #selector(controlChanged(_:))
        antialias.setAccessibilityLabel("Antialiasing")
        toleranceSlider.isContinuous = true
        toleranceSlider.target = self
        toleranceSlider.action = #selector(toleranceSlid(_:))
        toleranceSlider.translatesAutoresizingMaskIntoConstraints = false
        // The status bar's zoom slider size (docs/design/SPEC.md): 96 pt wide, 28 pt to hit.
        toleranceSlider.widthAnchor.constraint(equalToConstant: 96).isActive = true
        toleranceSlider.heightAnchor.constraint(equalToConstant: 28).isActive = true
        toleranceSlider.setAccessibilityLabel("Tolerance")
        toleranceAlpha.setAccessibilityLabel("Tolerance alpha")
        toleranceAlpha.toolTip = "Premultiplied: transparent pixels match. Straight: they match only when their colors match."
        floodFinish.bezelStyle = .rounded
        floodFinish.font = Theme.font(12)
        floodFinish.target = self
        floodFinish.action = #selector(floodFinishClicked(_:))
        floodFinish.isEnabled = false
        floodFinish.setAccessibilityLabel("Finish fill or selection")
        floodFinish.toolTip = "Finish (Enter): keep the result; Escape undoes the click"
        for p in [blend, selectionMode, sampling, floodMode, toleranceAlpha, pickerSize, pickerSampling,
                  pickerAfterClick] {
            p.target = self
            p.action = #selector(controlChanged(_:))
        }
        blend.setAccessibilityLabel("Blend mode")
        selectionMode.setAccessibilityLabel("Selection mode")
        sampling.setAccessibilityLabel("Sampling")
        floodMode.setAccessibilityLabel("Flood mode")
        pickerSize.setAccessibilityLabel("Sample size")
        pickerSampling.setAccessibilityLabel("Sampling")
        pickerAfterClick.setAccessibilityLabel("After click")
        tools.observe(self) { [weak self] in self?.sync() }
        sync()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func layout() {
        super.layout()
        var x: CGFloat = 0
        if let commandBar {
            x = ceil(commandBar.fittingSize.width)
            commandBar.frame = NSRect(x: 0, y: 0, width: x, height: bounds.height - 1)
        }
        scroll.frame = NSRect(x: x, y: 0, width: max(bounds.width - x, 0), height: bounds.height - 1)
    }

    private func setUpPicker() {
        picker.removeAllItems()
        for t in PaintTool.allCases {
            picker.addItem(withTitle: t.label)
            let config = NSImage.SymbolConfiguration(pointSize: 14, weight: .regular)
            picker.lastItem?.image = NSImage(systemSymbolName: t.symbol, accessibilityDescription: nil)?
                .withSymbolConfiguration(config)
            picker.lastItem?.representedObject = t.rawValue
        }
        picker.font = Theme.font(12)
        picker.target = self
        picker.action = #selector(pickTool(_:))
        picker.setAccessibilityLabel("Tool")
        picker.translatesAutoresizingMaskIntoConstraints = false
        picker.widthAnchor.constraint(equalToConstant: 170).isActive = true
    }

    /// Rebuilds the row for a new tool, then copies the settings into the controls.
    func sync() {
        let tool = tools.tool
        if tool != shownTool {
            shownTool = tool
            picker.selectItem(at: PaintTool.allCases.firstIndex(of: tool) ?? 0)
        }
        if tool.options != shownSet || stack.arrangedSubviews.isEmpty {
            shownSet = tool.options
            rebuild(tool.options)
        }
        let s = tools.settings
        if widthField.currentEditor() == nil { widthField.stringValue = Self.number(s.brushWidth) }
        if opacityField.currentEditor() == nil { opacityField.stringValue = Self.number(s.opacity) }
        if hardnessField.currentEditor() == nil { hardnessField.stringValue = Self.number(s.hardness) }
        pressureSize.state = s.pressureSize ? .on : .off
        pressureOpacity.state = s.pressureOpacity ? .on : .off
        if toleranceField.currentEditor() == nil { toleranceField.stringValue = Self.number(s.tolerance) }
        toleranceSlider.doubleValue = s.tolerance
        toleranceAlpha.selectItem(at: ToolSettings.ToleranceAlpha.allCases.firstIndex(of: s.toleranceAlpha) ?? 0)
        antialias.state = s.antialias ? .on : .off
        blend.selectItem(at: ToolSettings.Blend.allCases.firstIndex(of: s.blend) ?? 0)
        selectionMode.selectItem(at: ToolSettings.SelectionMode.allCases.firstIndex(of: s.selectionMode) ?? 0)
        sampling.selectItem(at: ToolSettings.Sampling.allCases.firstIndex(of: s.sampling) ?? 0)
        floodMode.selectItem(at: ToolSettings.FloodMode.allCases.firstIndex(of: s.floodMode) ?? 0)
        pickerSize.selectItem(at: PickerSampleSize.allCases.firstIndex(of: s.pickerSize) ?? 0)
        pickerSampling.selectItem(at: ToolSettings.Sampling.allCases.firstIndex(of: s.pickerSampling) ?? 0)
        pickerAfterClick.selectItem(at: PickerAfterClick.allCases.firstIndex(of: s.pickerAfterClick) ?? 0)
        minus.isEnabled = s.brushWidth > ToolSettings.widthRange.lowerBound
        plus.isEnabled = s.brushWidth < ToolSettings.widthRange.upperBound
        if tool.options == .text { textControls.sync() }
        // Paint Bucket uses blend mode; Magic Wand uses selection mode.
        if tool.options == .flood {
            selectionMode.isHidden = tool != .magicWand
            modeLabel?.stringValue = tool == .magicWand
                ? L10n.tr("Selection mode", "Tool options row: label") : L10n.tr("Blend mode", "Tool options row: label")
            blendInFlood?.isHidden = tool == .magicWand
        }
    }

    private var modeLabel: NSTextField?
    private var blendInFlood: NSView?

    private func rebuild(_ set: PaintTool.OptionSet) {
        for v in stack.arrangedSubviews {
            stack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        modeLabel = nil
        blendInFlood = nil
        func add(_ views: NSView...) { for v in views { stack.addArrangedSubview(v) } }
        func group() { add(Labels.gap(8)) }  // 8 + 8 stack spacing = 16 pt between groups
        add(picker)
        group()
        if let customOptions {
            for v in customOptions { add(v) }
        } else {
        switch set {
        case .none:
            add(Labels.label("This tool has no options."))
        case .brush, .eraser:
            add(Labels.label("Brush width"), minus, widthField, plus, Labels.label("px"))
            group()
            add(Labels.label("Hardness"), hardnessField, Labels.label("%"))
            group()
            add(Labels.label("Antialiasing"), antialias)
            group()
            if set == .brush {
                add(Labels.label("Blend mode"), blend)
                group()
            }
            add(Labels.label("Opacity"), opacityField, Labels.label("%"))
            group()
            add(Labels.label("Pen pressure"), pressureSize, pressureOpacity)
        case .pencil:
            add(Labels.label("Blend mode"), blend)
            group()
            add(Labels.label("Opacity"), opacityField, Labels.label("%"))
            group()
            add(Labels.label("Pen pressure"), pressureOpacity)
        case .flood:
            add(Labels.label("Tolerance"), toleranceSlider, toleranceField, Labels.label("%"))
            group()
            add(Labels.label("Tolerance alpha"), toleranceAlpha)
            group()
            let label = Labels.label("Selection mode")
            modeLabel = label
            add(label, selectionMode, blend)
            blendInFlood = blend
            group()
            add(Labels.label("Sampling"), sampling)
            group()
            add(Labels.label("Flood mode"), floodMode)
            group()
            add(Labels.label("Antialiasing"), antialias)
            group()
            add(floodFinish)
        case .picker:
            add(Labels.label("Sampling"), pickerSize)
            group()
            add(Labels.label("Layer or image"), pickerSampling)
            group()
            add(Labels.label("After click"), pickerAfterClick)
        case .text:
            textControls.add({ for v in $0 { stack.addArrangedSubview(v) } }, group: group, antialias: antialias,
                             blend: blend)
        }
        }
        for v in stack.arrangedSubviews where v is NSPopUpButton || v is NSButton {
            let has = v.constraints.contains { $0.firstAttribute == .height && $0.firstItem === v }
            if !has { v.heightAnchor.constraint(equalToConstant: 28).isActive = true }
        }
        L10n.localize(stack, context: "Tool options row")  // M59: the new row's words
        L10n.growWidthConstraints(in: stack)
    }

    static func number(_ v: Double) -> String {
        v.rounded() == v ? String(Int(v)) : String(format: "%.1f", v)
    }

    @objc private func pickTool(_ sender: NSPopUpButton) {
        guard let raw = sender.selectedItem?.representedObject as? String, let t = PaintTool(rawValue: raw) else { return }
        tools.tool = t
    }

    @objc private func stepWidth(_ sender: NSButton) {
        tools.settings.stepWidth(up: sender === plus)
    }

    @objc private func fieldChanged(_ sender: NSTextField) {
        let v = Double(sender.stringValue.trimmingCharacters(in: .whitespaces)) ?? .nan
        var s = tools.settings
        if sender === widthField, v.isFinite {
            s.brushWidth = min(max(v, ToolSettings.widthRange.lowerBound), ToolSettings.widthRange.upperBound)
        } else if sender === opacityField, v.isFinite {
            s.opacity = min(max(v, 0), 100)
        } else if sender === toleranceField, v.isFinite {
            s.tolerance = min(max(v, 0), 100)
        } else if sender === hardnessField, v.isFinite {
            s.hardness = min(max(v, 0), 100)
        }
        tools.settings = s
        sync()  // shows the clamped value, or the old one after bad input
    }

    /// Whole percents, as paint.net's tolerance slider shows them.
    @objc private func toleranceSlid(_ sender: NSSlider) {
        tools.settings.tolerance = min(max(sender.doubleValue.rounded(), 0), 100)
    }

    @objc private func floodFinishClicked(_ sender: Any?) { onFloodFinish?() }

    @objc private func controlChanged(_ sender: Any?) {
        var s = tools.settings
        s.antialias = antialias.state == .on
        s.pressureSize = pressureSize.state == .on
        s.pressureOpacity = pressureOpacity.state == .on
        s.blend = ToolSettings.Blend.allCases[max(blend.indexOfSelectedItem, 0)]
        s.selectionMode = ToolSettings.SelectionMode.allCases[max(selectionMode.indexOfSelectedItem, 0)]
        s.sampling = ToolSettings.Sampling.allCases[max(sampling.indexOfSelectedItem, 0)]
        s.floodMode = ToolSettings.FloodMode.allCases[max(floodMode.indexOfSelectedItem, 0)]
        s.toleranceAlpha = ToolSettings.ToleranceAlpha.allCases[max(toleranceAlpha.indexOfSelectedItem, 0)]
        s.pickerSize = PickerSampleSize.allCases[max(pickerSize.indexOfSelectedItem, 0)]
        s.pickerSampling = ToolSettings.Sampling.allCases[max(pickerSampling.indexOfSelectedItem, 0)]
        s.pickerAfterClick = PickerAfterClick.allCases[max(pickerAfterClick.indexOfSelectedItem, 0)]
        tools.settings = s
    }
}
