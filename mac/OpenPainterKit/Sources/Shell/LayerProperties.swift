// Layer Properties (M20): name, visible, blend mode (all 14) and opacity (a
// slider and a 0 to 255 field) as an engine preview session. Each change
// updates the preview, so the canvas shows it live. OK commits one history
// step; Cancel ends the session and leaves the document and history as they
// were. The layer actions of the Layers menu and panel are here too.
import AppKit

/// Layers menu selectors the document window controller implements.
@MainActor @objc public protocol LayerMenuActions {
    func toggleLayerVisibility(_ sender: Any?)
    func showLayerProperties(_ sender: Any?)
}

@MainActor
public final class LayerPropertiesController: NSWindowController, NSTextFieldDelegate {
    public struct Values: Equatable, Sendable {
        public var name: String
        public var visible: Bool
        public var blend: String
        public var opacity: Int
    }

    public let editor: Editor
    public let index: Int
    public let original: Values
    public private(set) var values: Values
    /// True once the engine session has begun (on the first change).
    public private(set) var sessionStarted = false
    /// Called when the sheet closes, with true for OK.
    var onClose: ((Bool) -> Void)?

    public let nameField = NSTextField(string: "")
    public let visibleBox = NSButton(checkboxWithTitle: "Visible", target: nil, action: nil)
    public let blendPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    public let opacitySlider = NSSlider(value: 255, minValue: 0, maxValue: 255, target: nil, action: nil)
    public let opacityField = NSTextField(string: "")
    public private(set) var okButton: NSButton!
    public private(set) var cancelButton: NSButton!

    /// The open dialog for each editor (one at a time).
    static var open: [ObjectIdentifier: LayerPropertiesController] = [:]
    public static func current(for editor: Editor) -> LayerPropertiesController? { open[ObjectIdentifier(editor)] }

    public init(editor: Editor, index: Int) {
        self.editor = editor
        self.index = index
        let l = editor.info.layers[index]
        original = Values(name: l.name, visible: l.visible, blend: l.blend, opacity: l.opacity)
        values = original
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 340, height: 214), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = "Layer Properties"
        super.init(window: panel)
        build(panel)
        show(values)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func build(_ panel: NSPanel) {
        let content = FilledView(fill: Theme.panel)
        panel.contentView = content
        func label(_ text: String, _ y: CGFloat) {
            let l = Labels.label(text, size: 12, color: Theme.ink)
            l.alignment = .right
            l.frame = NSRect(x: 16, y: y + 4, width: 84, height: 16)
            content.addSubview(l)
        }
        // y grows down (FilledView is flipped). 28 pt rows, 12 pt apart.
        label("Name:", 20)
        nameField.frame = NSRect(x: 108, y: 20, width: 216, height: 24)
        nameField.font = Theme.font(12)
        nameField.delegate = self
        nameField.setAccessibilityLabel("Layer name")
        content.addSubview(nameField)

        visibleBox.frame = NSRect(x: 106, y: 54, width: 120, height: 24)
        visibleBox.font = Theme.font(12)
        visibleBox.target = self
        visibleBox.action = #selector(changed(_:))
        content.addSubview(visibleBox)

        label("Blend mode:", 86)
        blendPopup.addItems(withTitles: LayerBlend.modes.map(\.title))
        blendPopup.font = Theme.font(12)
        blendPopup.frame = NSRect(x: 106, y: 84, width: 160, height: 28)
        blendPopup.target = self
        blendPopup.action = #selector(changed(_:))
        blendPopup.setAccessibilityLabel("Blend mode")
        content.addSubview(blendPopup)

        label("Opacity:", 122)
        opacitySlider.frame = NSRect(x: 108, y: 120, width: 150, height: 28)
        opacitySlider.isContinuous = true
        opacitySlider.numberOfTickMarks = 0
        opacitySlider.target = self
        opacitySlider.action = #selector(sliderMoved(_:))
        opacitySlider.setAccessibilityLabel("Opacity")
        content.addSubview(opacitySlider)
        let f = NumberFormatter()
        f.minimum = 0
        f.maximum = 255
        f.allowsFloats = false
        opacityField.formatter = f
        opacityField.font = Theme.font(12)
        opacityField.alignment = .right
        opacityField.frame = NSRect(x: 268, y: 122, width: 56, height: 24)
        opacityField.delegate = self
        opacityField.target = self
        opacityField.action = #selector(opacityTyped(_:))
        opacityField.setAccessibilityLabel("Opacity value, 0 to 255")
        content.addSubview(opacityField)

        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        cancelButton.frame = NSRect(x: 160, y: 170, width: 80, height: 28)
        okButton = NSButton(title: "OK", target: self, action: #selector(ok(_:)))
        okButton.keyEquivalent = "\r"
        okButton.frame = NSRect(x: 244, y: 170, width: 80, height: 28)
        content.addSubview(cancelButton)
        content.addSubview(okButton)
    }

    private func show(_ v: Values) {
        if nameField.currentEditor() == nil { nameField.stringValue = v.name }
        visibleBox.state = v.visible ? .on : .off
        blendPopup.selectItem(at: LayerBlend.modes.firstIndex { $0.id == v.blend } ?? 0)
        opacitySlider.integerValue = v.opacity
        if opacityField.currentEditor() == nil { opacityField.integerValue = v.opacity }
    }

    /// Opens the dialog as a sheet on the editor's window.
    public func present(on window: NSWindow) {
        Self.open[ObjectIdentifier(editor)] = self
        L10n.localizeDialog(self.window!)  // M59: translated and named before it shows
        window.beginSheet(self.window!) { _ in }
        self.window?.makeFirstResponder(nameField)
    }

    // MARK: Changes

    /// Sets the values and updates the preview (also used by tests).
    public func set(_ v: Values) {
        var v = v
        v.opacity = min(max(v.opacity, 0), 255)
        guard v != values else { return }
        values = v
        show(v)
        preview()
    }

    private func preview() {
        let params: [String: any Sendable] = [
            "index": index, "name": values.name, "visible": values.visible, "blend": values.blend,
            "opacity": values.opacity,
        ]
        if sessionStarted {
            editor.run("preview.update", params)
        } else {
            sessionStarted = true
            editor.run("preview.begin", ["command": "layer.properties", "params": params])
        }
    }

    @objc private func changed(_ sender: Any?) {
        var v = values
        v.visible = visibleBox.state == .on
        v.blend = LayerBlend.modes[max(blendPopup.indexOfSelectedItem, 0)].id
        set(v)
    }

    @objc private func sliderMoved(_ sender: NSSlider) {
        var v = values
        v.opacity = sender.integerValue
        opacityField.integerValue = v.opacity
        set(v)
    }

    @objc private func opacityTyped(_ sender: NSTextField) {
        var v = values
        v.opacity = sender.integerValue
        set(v)
    }

    public func controlTextDidChange(_ note: Notification) {
        guard let field = note.object as? NSTextField else { return }
        var v = values
        if field === nameField {
            v.name = field.stringValue
        } else if field === opacityField, let n = Int(field.stringValue) {
            v.opacity = n
            opacitySlider.integerValue = min(max(n, 0), 255)
        }
        set(v)
    }

    // MARK: OK and Cancel

    @objc public func ok(_ sender: Any?) {
        if sessionStarted {
            // A session whose values ended where they started has nothing to commit.
            editor.run(values == original ? "preview.cancel" : "preview.commit")
        }
        close(true)
    }

    @objc public func cancel(_ sender: Any?) {
        if sessionStarted { editor.run("preview.cancel") }
        close(false)
    }

    private func close(_ ok: Bool) {
        Self.open[ObjectIdentifier(editor)] = nil
        if let sheet = window, let parent = sheet.sheetParent {
            parent.endSheet(sheet)
        }
        window?.orderOut(nil)
        onClose?(ok)
    }
}

// MARK: Layer actions on the document window

extension EditorWindowController: LayerMenuActions {
    @objc public func toggleLayerVisibility(_ sender: Any?) {
        let i = editor.info.activeLayer
        guard i < editor.info.layers.count else { return }
        editor.run("layer.set_visible", ["index": i, "visible": !editor.info.layers[i].visible])
    }

    @objc public func showLayerProperties(_ sender: Any?) {
        showLayerProperties(index: editor.info.activeLayer)
    }

    /// Opens Layer Properties for the layer at `index` (the active layer from
    /// the menu, a double-clicked row from the panel).
    @discardableResult
    public func showLayerProperties(index: Int) -> LayerPropertiesController? {
        guard let window, window.attachedSheet == nil, index < editor.info.layers.count else { return nil }
        let dialog = LayerPropertiesController(editor: editor, index: index)
        dialog.onClose = { [weak self] _ in self?.reloadPanels() }
        dialog.present(on: window)
        reloadPanels()
        return dialog
    }

    /// The panels turn their commands off while the sheet is up, and back on.
    private func reloadPanels() {
        workspace.layersView.reload()
        workspace.historyView.reload()
    }
}
