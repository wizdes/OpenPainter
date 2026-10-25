// The 28 pt status bar: the tool hint on the left; selection size, cursor
// position, image size, units, an editable zoom, Fit, 100% and a 96 pt zoom
// slider on the right. On a tight window the hint hides first.
import AppKit

@MainActor
public final class StatusBarView: FilledView {
    public let hint = Labels.label("", color: Theme.muted)
    public let selectionValue = Labels.label("", color: Theme.ink)
    public let cursorValue = Labels.label("", color: Theme.ink)
    public let imageValue = Labels.label("", color: Theme.ink)
    public let units = NSPopUpButton(frame: .zero, pullsDown: false)
    public let zoomField = Labels.field("100%", width: 56, height: 24)
    public private(set) var fitButton: NSButton!
    public private(set) var actualButton: NSButton!
    public let slider = NSSlider(value: 0, minValue: log(ZoomPresets.minimum), maxValue: log(ZoomPresets.maximum),
                                 target: nil, action: nil)
    private let right = NSStackView()

    /// Called with a new zoom (field, slider) or nil for Fit.
    public var onZoom: ((Double?) -> Void)?

    public init() {
        super.init(fill: Theme.chrome, dividers: [.top])
        frame = NSRect(x: 0, y: 0, width: 1100, height: Theme.statusHeight)
        fitButton = textButton("Fit", #selector(fit(_:)))
        actualButton = textButton("100%", #selector(actual(_:)))
        units.addItems(withTitles: ["px", "in", "cm"])
        units.item(at: 1)?.isEnabled = false
        units.item(at: 2)?.isEnabled = false
        units.autoenablesItems = false
        units.font = Theme.font(12)
        units.controlSize = .small
        units.toolTip = "Units (inches and centimeters come later)"
        units.setAccessibilityLabel("Units")
        zoomField.target = self
        zoomField.action = #selector(zoomTyped(_:))
        zoomField.setAccessibilityLabel("Zoom")
        slider.target = self
        slider.action = #selector(slid(_:))
        slider.controlSize = .small
        slider.setAccessibilityLabel("Zoom slider")
        slider.translatesAutoresizingMaskIntoConstraints = false
        slider.widthAnchor.constraint(equalToConstant: 96).isActive = true
        slider.heightAnchor.constraint(equalToConstant: 28).isActive = true
        right.orientation = .horizontal
        right.alignment = .centerY
        right.spacing = 10
        for (label, value) in [("Selection", selectionValue), ("Cursor", cursorValue), ("Image", imageValue)] {
            let l = Labels.label(label)
            right.addArrangedSubview(l)
            right.addArrangedSubview(value)
            right.setCustomSpacing(4, after: l)
            value.setAccessibilityLabel(label)
        }
        let u = Labels.label("Units")
        right.addArrangedSubview(u)
        right.setCustomSpacing(4, after: u)
        for v in [units, zoomField, fitButton!, actualButton!, slider] as [NSView] { right.addArrangedSubview(v) }
        right.translatesAutoresizingMaskIntoConstraints = false
        hint.translatesAutoresizingMaskIntoConstraints = false
        addSubview(hint)
        addSubview(right)
        let squeeze = hint.trailingAnchor.constraint(lessThanOrEqualTo: right.leadingAnchor, constant: -10)
        squeeze.priority = .defaultHigh  // a very narrow window drops the hint, not the values
        NSLayoutConstraint.activate([
            hint.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            hint.centerYAnchor.constraint(equalTo: centerYAnchor),
            right.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            right.centerYAnchor.constraint(equalTo: centerYAnchor),
            squeeze,
        ])
        hint.setContentCompressionResistancePriority(.init(1), for: .horizontal)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func textButton(_ title: String, _ action: Selector) -> NSButton {
        let b = NSButton(title: title, target: self, action: action)
        b.bezelStyle = .push
        b.controlSize = .small
        b.font = Theme.font(12)
        b.translatesAutoresizingMaskIntoConstraints = false
        b.heightAnchor.constraint(greaterThanOrEqualToConstant: 24).isActive = true
        return b
    }

    public override func layout() {
        super.layout()
        // Hide the hint before squeezing any value.
        hint.isHidden = right.fittingSize.width + 40 > bounds.width
    }

    /// Updates every value. `cursor` and `selection` are in document pixels.
    public func update(selection: CGRect?, cursor: CGPoint?, imageSize: CGSize, zoom: Double, hint text: String) {
        selectionValue.stringValue = selection.map { "\(Int($0.width)) × \(Int($0.height)) px" } ?? "None"
        cursorValue.stringValue = cursor.map { "\(Int($0.x.rounded(.down))), \(Int($0.y.rounded(.down)))" } ?? "–"
        imageValue.stringValue = "\(Int(imageSize.width)) × \(Int(imageSize.height)) px"
        if zoomField.currentEditor() == nil { zoomField.stringValue = ZoomPresets.label(zoom) }
        slider.doubleValue = log(zoom)
        hint.stringValue = text
    }

    @objc private func fit(_ sender: Any?) { onZoom?(nil) }
    @objc private func actual(_ sender: Any?) { onZoom?(1) }

    @objc private func zoomTyped(_ sender: NSTextField) {
        let text = sender.stringValue.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespaces)
        guard let percent = Double(text), percent.isFinite, percent > 0 else {
            sender.stringValue = ""  // refilled with the real zoom by the next update
            onZoom?(-1)
            return
        }
        onZoom?(ZoomPresets.clamp(percent / 100))
    }

    @objc private func slid(_ sender: NSSlider) {
        onZoom?(ZoomPresets.clamp(exp(sender.doubleValue)))
    }
}
