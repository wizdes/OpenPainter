// Image > Color Profile… (M27). The sheet shows the document's profile (or
// "None, treated as sRGB") and runs one engine command on OK:
//   Assign  image.assign_profile   changes the label only; pixel bytes stay.
//   Convert image.convert_profile  changes the pixels into the target profile,
//                                  with a rendering intent and black point
//                                  compensation (op-color's lcms2 flags).
// Targets: the four built-in profiles paint.net 5.1 lists (sRGB, Adobe RGB,
// Display P3, ProPhoto RGB), the profile of the window's display, and any
// .icc/.icm file from disk (paint.net Image menu, "Color Profile",
// https://www.getpaint.net/doc/latest/ImageMenu.html). Each OK is one history
// step; Cancel runs nothing. The display path (MetalCanvasView) follows the
// window's screen by itself and never touches the document.
import AppKit
import UniformTypeIdentifiers

/// The Image menu selector the document window controller implements.
@MainActor @objc public protocol ColorProfileMenuActions {
    func showColorProfile(_ sender: Any?)
}

/// Names for the status bar and the dialog.
public enum ColorProfileNames {
    public static let noneTitle = "None, treated as sRGB"
    /// Longest status bar name before it is cut with "…".
    static let shortLimit = 18

    public static func full(_ info: DocInfo) -> String {
        info.profileEmbedded ? info.profileDescription : noneTitle
    }

    public static func short(_ info: DocInfo) -> String {
        guard info.profileEmbedded else { return "sRGB (none)" }
        let name = info.profileDescription.trimmingCharacters(in: .whitespaces)
        if name.count <= shortLimit { return name }
        return String(name.prefix(shortLimit - 1)).trimmingCharacters(in: .whitespaces) + "…"
    }
}

@MainActor
public final class ColorProfileController: NSWindowController {
    public enum Action: Equatable, Sendable { case convert, assign }

    public enum Target: Equatable, Sendable {
        /// An op-color built-in: its engine name and title.
        case builtin(id: String, title: String)
        /// ICC bytes: the window's display or a file.
        case icc(Data, title: String)

        public var title: String {
            switch self {
            case let .builtin(_, t), let .icc(_, t): return t
            }
        }

        /// The engine's `profile` parameter.
        var param: String {
            switch self {
            case let .builtin(id, _): return id
            case let .icc(data, _): return "base64:" + data.base64EncodedString()
            }
        }
    }

    public struct Values: Equatable, Sendable {
        public var action: Action
        public var target: Target
        /// The engine intent name: perceptual, relative, saturation, absolute.
        public var intent: String
        public var blackPoint: Bool
    }

    /// The built-in profiles, in the popup's order.
    public static let builtins: [Target] = [
        .builtin(id: "srgb", title: "sRGB"), .builtin(id: "display-p3", title: "Display P3"),
        .builtin(id: "adobe-rgb", title: "Adobe RGB (1998)"), .builtin(id: "prophoto", title: "ProPhoto RGB"),
    ]
    public static let intents: [(id: String, title: String)] = [
        ("perceptual", "Perceptual"), ("relative", "Relative colorimetric"), ("saturation", "Saturation"),
        ("absolute", "Absolute colorimetric"),
    ]
    static let fileItemTitle = "ICC File…"

    public let editor: Editor
    /// The document profile when the dialog opened.
    public let current: (embedded: Bool, description: String, space: String, iccLength: Int)
    public private(set) var values: Values
    /// The profile of the window's display, when it has one.
    public let display: Target?
    /// The ICC file loaded last, shown above "ICC File…".
    public private(set) var file: Target?
    /// Called when the sheet closes, with true for OK.
    var onClose: ((Bool) -> Void)?

    public let currentName = Labels.label("", size: 12, color: Theme.ink, weight: .semibold)
    public let currentDetail = Labels.label("")
    public let convertRadio = NSButton(radioButtonWithTitle: "Convert: change pixels so colors look the same",
                                       target: nil, action: nil)
    public let assignRadio = NSButton(radioButtonWithTitle: "Assign: keep pixels, change only the profile",
                                      target: nil, action: nil)
    public let targetPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    public let intentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    public let blackPointBox = NSButton(checkboxWithTitle: "Black point compensation", target: nil, action: nil)
    /// The explanation of the chosen action, or an error (in `accent`).
    public let note = NSTextField(wrappingLabelWithString: "")
    public private(set) var okButton: NSButton!
    public private(set) var cancelButton: NSButton!
    private var intentLabel: NSTextField!

    /// The open dialog for each editor (one at a time).
    static var open: [ObjectIdentifier: ColorProfileController] = [:]
    public static func current(for editor: Editor) -> ColorProfileController? { open[ObjectIdentifier(editor)] }

    public init(editor: Editor, displayICC: Data?, displayName: String?) {
        self.editor = editor
        var q: [String: Any] = [:]
        do {
            q = try editor.document.query("doc.profile") as? [String: Any] ?? [:]
        } catch {
            // Falls back to doc.info's name; the color space then reads RGB.
            editor.report("could not read the color profile: \(error)")
        }
        let now = (
            embedded: q["embedded"] as? Bool ?? editor.info.profileEmbedded,
            description: q["description"] as? String ?? editor.info.profileDescription,
            space: q["color_space"] as? String ?? "RGB",
            iccLength: (q["icc_len"] as? NSNumber)?.intValue ?? 0
        )
        current = now
        display = displayICC.map { .icc($0, title: "This display: \(displayName ?? "Display")") }
        // Default target: the first built-in that differs from the current profile.
        let currentTitle = now.embedded ? now.description : "sRGB"
        let start = Self.builtins.first { $0.title != currentTitle }!
        values = Values(action: .convert, target: start, intent: "perceptual", blackPoint: false)
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 440, height: 336), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = "Color Profile"
        super.init(window: panel)
        build(panel)
        show()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    // MARK: Layout

    private func build(_ panel: NSPanel) {
        let content = FilledView(fill: Theme.panel)
        panel.contentView = content
        @discardableResult
        func label(_ text: String, _ y: CGFloat) -> NSTextField {
            let l = Labels.label(text, size: 12, color: Theme.ink)
            l.alignment = .right
            l.frame = NSRect(x: 16, y: y + 4, width: 84, height: 16)
            content.addSubview(l)
            return l
        }
        // y grows down (FilledView is flipped). Rows are 24 to 28 pt, 8 to 12 pt apart.
        label("Current:", 16)
        currentName.frame = NSRect(x: 108, y: 20, width: 316, height: 16)
        currentName.setAccessibilityLabel("Current profile")
        currentDetail.frame = NSRect(x: 108, y: 38, width: 316, height: 16)
        content.addSubview(currentName)
        content.addSubview(currentDetail)

        label("Action:", 66)
        for (radio, y) in [(convertRadio, 66.0), (assignRadio, 92.0)] {
            radio.frame = NSRect(x: 106, y: y, width: 320, height: 24)
            radio.font = Theme.font(12)
            radio.target = self
            radio.action = #selector(actionChanged(_:))
            content.addSubview(radio)
        }

        label("Profile:", 128)
        targetPopup.font = Theme.font(12)
        targetPopup.frame = NSRect(x: 106, y: 126, width: 300, height: 28)
        targetPopup.autoenablesItems = false
        targetPopup.target = self
        targetPopup.action = #selector(targetChanged(_:))
        targetPopup.setAccessibilityLabel("Target profile")
        content.addSubview(targetPopup)

        intentLabel = label("Intent:", 164)
        intentPopup.addItems(withTitles: Self.intents.map(\.title))
        intentPopup.font = Theme.font(12)
        intentPopup.frame = NSRect(x: 106, y: 162, width: 200, height: 28)
        intentPopup.target = self
        intentPopup.action = #selector(optionsChanged(_:))
        intentPopup.setAccessibilityLabel("Rendering intent")
        content.addSubview(intentPopup)

        blackPointBox.frame = NSRect(x: 106, y: 198, width: 260, height: 24)
        blackPointBox.font = Theme.font(12)
        blackPointBox.target = self
        blackPointBox.action = #selector(optionsChanged(_:))
        content.addSubview(blackPointBox)

        note.font = Theme.font(12)
        note.textColor = Theme.muted
        note.frame = NSRect(x: 108, y: 232, width: 316, height: 44)
        note.setAccessibilityLabel("Note")
        content.addSubview(note)

        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        cancelButton.frame = NSRect(x: 260, y: 292, width: 80, height: 28)
        okButton = NSButton(title: "OK", target: self, action: #selector(ok(_:)))
        okButton.keyEquivalent = "\r"
        okButton.frame = NSRect(x: 344, y: 292, width: 80, height: 28)
        content.addSubview(cancelButton)
        content.addSubview(okButton)
    }

    /// Every popup entry that picks a target, in order (nil for a separator).
    private var targetEntries: [Target?] {
        var list: [Target?] = Self.builtins
        if let display { list += [nil, display] }
        if let file { list += [nil, file] }
        return list
    }

    private func rebuildTargets() {
        targetPopup.removeAllItems()
        for entry in targetEntries {
            if let entry {
                targetPopup.addItem(withTitle: entry.title)
            } else {
                targetPopup.menu?.addItem(.separator())
            }
        }
        targetPopup.menu?.addItem(.separator())
        targetPopup.addItem(withTitle: Self.fileItemTitle)
        if let i = targetEntries.firstIndex(where: { $0 == values.target }) { targetPopup.selectItem(at: i) }
    }

    private func show(error: String? = nil) {
        currentName.stringValue = current.embedded ? current.description : ColorProfileNames.noneTitle
        currentDetail.stringValue = current.embedded
            ? "Color space \(current.space), embedded ICC profile (\(current.iccLength) bytes)"
            : "Color space RGB. The image has no embedded profile."
        convertRadio.state = values.action == .convert ? .on : .off
        assignRadio.state = values.action == .assign ? .on : .off
        rebuildTargets()
        let converting = values.action == .convert
        intentPopup.isEnabled = converting
        blackPointBox.isEnabled = converting
        intentLabel.textColor = converting ? Theme.ink : Theme.muted
        intentPopup.selectItem(at: Self.intents.firstIndex { $0.id == values.intent } ?? 0)
        blackPointBox.state = values.blackPoint ? .on : .off
        if let error {
            note.stringValue = error
            note.textColor = Theme.accent
        } else {
            note.stringValue = converting
                ? "Converts every layer into \(values.target.title). Colors stay the same; pixel values change."
                : "Labels the image as \(values.target.title). Pixel values stay the same; colors can look different."
            note.textColor = Theme.muted
        }
    }

    /// Opens the dialog as a sheet on the editor's window.
    public func present(on window: NSWindow) {
        Self.open[ObjectIdentifier(editor)] = self
        window.beginSheet(self.window!) { _ in }
        self.window?.makeFirstResponder(targetPopup)
    }

    // MARK: Changes

    /// Sets the values (also used by tests).
    public func set(_ v: Values) {
        values = v
        show()
    }

    @objc private func actionChanged(_ sender: NSButton) {
        values.action = sender === assignRadio ? .assign : .convert
        show()
    }

    @objc private func optionsChanged(_ sender: Any?) {
        values.intent = Self.intents[max(intentPopup.indexOfSelectedItem, 0)].id
        values.blackPoint = blackPointBox.state == .on
        show()
    }

    @objc private func targetChanged(_ sender: NSPopUpButton) {
        let i = sender.indexOfSelectedItem
        let entries = targetEntries
        if i >= 0, i < entries.count, let t = entries[i] {
            values.target = t
            show()
        } else {
            show()  // reselects the current target while the file panel is up
            chooseFile()
        }
    }

    /// Opens an .icc or .icm file as a sheet on this dialog.
    private func chooseFile() {
        guard let sheet = window else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.allowedContentTypes = ["icc", "icm"].compactMap { UTType(filenameExtension: $0) }
        panel.beginSheetModal(for: sheet) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.loadFile(url) }
        }
    }

    /// Reads an ICC file and makes it the target. A file that is not an RGB
    /// ICC profile shows an error in the note and changes nothing.
    @discardableResult
    public func loadFile(_ url: URL) -> Bool {
        let data: Data
        do {
            data = try Data(contentsOf: url)
        } catch {
            show(error: "Could not read \(url.lastPathComponent): \(error.localizedDescription)")
            return false
        }
        if let problem = Self.iccProblem(data) {
            show(error: "\(url.lastPathComponent) is not usable: \(problem)")
            return false
        }
        let name = CGColorSpace(iccData: data as CFData)?.name.map { $0 as String }
        let target = Target.icc(data, title: "File: \(url.lastPathComponent)" + (name.map { " (\($0))" } ?? ""))
        file = target
        values.target = target
        show()
        return true
    }

    /// Why `data` cannot be a document profile, or nil when it can.
    nonisolated static func iccProblem(_ data: Data) -> String? {
        let b = [UInt8](data)
        guard b.count >= 132, b[36..<40] == [0x61, 0x63, 0x73, 0x70] else { return "not an ICC profile" }
        let size = b[0..<4].reduce(0) { $0 << 8 | Int($1) }
        guard size == b.count else { return "the ICC size does not match the file" }
        guard b[16..<20] == [0x52, 0x47, 0x42, 0x20] else { return "it is not an RGB profile" }
        guard CGColorSpace(iccData: data as CFData) != nil else { return "ColorSync cannot read it" }
        return nil
    }

    // MARK: OK and Cancel

    /// The engine command and parameters OK runs.
    public var command: (id: String, params: [String: any Sendable]) {
        switch values.action {
        case .assign:
            return ("image.assign_profile", ["profile": values.target.param])
        case .convert:
            return ("image.convert_profile",
                    ["profile": values.target.param, "intent": values.intent, "bpc": values.blackPoint])
        }
    }

    @objc public func ok(_ sender: Any?) {
        let c = command
        editor.run(c.id, c.params)
        close(true)
    }

    @objc public func cancel(_ sender: Any?) {
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

// MARK: The menu action on the document window

extension EditorWindowController: ColorProfileMenuActions {
    @objc public func showColorProfile(_ sender: Any?) {
        showColorProfile()
    }

    @discardableResult
    public func showColorProfile() -> ColorProfileController? {
        guard let window, window.attachedSheet == nil else { return nil }
        let space = (window.screen ?? NSScreen.main)?.colorSpace
        let dialog = ColorProfileController(
            editor: editor, displayICC: canvas.displayICCOverride ?? space?.cgColorSpace?.copyICCData() as Data?,
            displayName: space?.localizedName)
        dialog.onClose = { [weak self] _ in
            self?.workspace.layersView.reload()
            self?.workspace.historyView.reload()
        }
        dialog.present(on: window)
        workspace.layersView.reload()
        workspace.historyView.reload()
        return dialog
    }
}
