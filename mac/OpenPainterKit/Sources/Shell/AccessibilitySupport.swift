// VoiceOver support (M59): the canvas element, panel headings, the strings
// the window and its dialogs show, and the wiring the main window needs.
//
// AppKit APIs used (Apple, "NSAccessibility" and "Accessibility Programming
// Guide for OS X":
// https://developer.apple.com/documentation/appkit/nsaccessibility-swift.protocol,
// https://developer.apple.com/library/archive/documentation/Accessibility/Conceptual/AccessibilityMacOSX/):
// - `setAccessibilityElement`, `setAccessibilityRole`, `setAccessibilityLabel`,
//   `setAccessibilityValue`, `setAccessibilityHelp` on NSView: the role-based
//   API, so each view answers for itself.
// - `NSAccessibilityElement.element(withRole:frame:label:parent:)` for the
//   panel headings, which have no view of their own.
// - `NSAccessibility.post(element:notification:)` with `.valueChanged` when the
//   canvas description changes, and `.focusedWindowChanged` when the panel
//   cycle moves focus.
// - The heading role is "AXHeading" (`NSAccessibilityHeadingRole`, named in
//   the macOS 26 SDK; VoiceOver reads the same role in web content on older
//   systems). No heading level is set.
import AppKit

extension NSAccessibility.Role {
    /// VoiceOver's heading role ("AXHeading").
    static let heading = NSAccessibility.Role(rawValue: "AXHeading")
}

@MainActor
public enum PanelAccessibility {
    /// Adds a heading element with the panel's title at the top of each
    /// panel, translates what the panels show, and installs the sheet hook
    /// that translates dialogs when they open.
    public static func install(_ panels: PanelManager) {
        for kind in PanelKind.allCases {
            guard let panel = panels.panels[kind], let content = panel.contentView else { continue }
            let title = L10n.tr(kind.title, "Floating panel title")
            panel.title = title
            panel.setAccessibilityLabel(L10n.tr("\(kind.title) panel", "VoiceOver name of a floating panel"))
            let heading = PanelHeadingView(title: title)
            heading.frame = NSRect(x: 0, y: 0, width: 1, height: 1)
            heading.autoresizingMask = [.maxXMargin, content.isFlipped ? .maxYMargin : .minYMargin]
            content.addSubview(heading, positioned: .below, relativeTo: nil)
            panel.autorecalculatesKeyViewLoop = true  // rows come and go
            L10n.localize(content, context: "\(kind.title) panel")
            L10n.growWidthConstraints(in: content)
        }
        L10n.installSheetHook()
    }
}

/// An invisible heading that names a panel for VoiceOver ("Layers, heading").
public final class PanelHeadingView: NSView {
    public init(title: String) {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.heading)
        setAccessibilityLabel(title)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var acceptsFirstResponder: Bool { false }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

@MainActor
extension L10n {
    private static var sheetObserver: NSObjectProtocol?

    /// Translates each sheet (dialog) the first time it opens. Dialog builders
    /// set their English strings; this hook runs `localize` on the sheet's
    /// window before it is drawn.
    public static func installSheetHook() {
        guard sheetObserver == nil else { return }
        sheetObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.willBeginSheetNotification, object: nil, queue: .main
        ) { note in
            let parent = note.object as? NSWindow
            MainActor.assumeIsolated {
                // The sheet is attached right after this notification.
                DispatchQueue.main.async { MainActor.assumeIsolated { localizeSheet(of: parent) } }
                localizeSheet(of: parent)
            }
        }
    }

    /// Windows already translated, held weakly: a freed window leaves the set,
    /// so a new window at the same address is not skipped (an ObjectIdentifier
    /// set kept freed identities and skipped new dialogs).
    private static let done = NSHashTable<NSWindow>.weakObjects()

    static func localizeSheet(of parent: NSWindow?) {
        guard let sheet = parent?.attachedSheet, !done.contains(sheet) else { return }
        done.add(sheet)
        localize(sheet, context: sheet.title.isEmpty ? "Dialog" : "\(sheet.title) dialog")
        if let content = sheet.contentView { growWidthConstraints(in: content) }
        // NSAlert lays itself out; the others are frame-based dialogs.
        if !String(describing: type(of: sheet)).contains("Alert") { widenToFit(sheet) }
    }

    /// Translates a dialog window now (tests, and windows that are not sheets).
    public static func localizeDialog(_ window: NSWindow) {
        guard !done.contains(window) else { return }
        done.add(window)
        localize(window, context: window.title.isEmpty ? "Dialog" : "\(window.title) dialog")
        if let content = window.contentView { growWidthConstraints(in: content) }
        widenToFit(window)
    }
}

extension EditorWindowController {
    /// M59 wiring: canvas keys, the canvas's VoiceOver element, and the
    /// window's strings.
    func wireAccessibility() {
        canvas.onShortcutKey = { [weak self] e in self?.canvasShortcut(e) ?? false }
        canvas.setAccessibilityElement(true)
        canvas.setAccessibilityRole(.image)
        canvas.setAccessibilityHelp(L10n.tr(
            "Tool letters pick tools. Shift with a letter goes back. Bracket keys change the brush width.",
            "VoiceOver help for the canvas"))
        container.setAccessibilityLabel(L10n.tr("Canvas area", "VoiceOver name of the area around the image"))
        statusBar.setAccessibilityElement(true)
        statusBar.setAccessibilityRole(.group)
        statusBar.setAccessibilityLabel(L10n.tr("Status bar", "VoiceOver name of the status bar"))
        optionsBar.setAccessibilityElement(true)
        optionsBar.setAccessibilityRole(.group)
        optionsBar.setAccessibilityLabel(L10n.tr("Tool options", "VoiceOver name of the tool options row"))
        window?.autorecalculatesKeyViewLoop = true
        if let window {
            L10n.localize(window, context: "Main window")
            if let content = window.contentView { L10n.growWidthConstraints(in: content) }
        }
        updateCanvasAccessibility()
    }

    /// What VoiceOver says for the canvas: the image name, size, zoom and tool.
    public var canvasAccessibilityDescription: (label: String, value: String) {
        let label = String(format: L10n.tr("Image %@", "VoiceOver name of the canvas; %@ is the image name"),
                           editor.name)
        let value = String(
            format: L10n.tr("%d by %d pixels, zoom %@, tool %@",
                            "VoiceOver value of the canvas: width, height, zoom percent, current tool"),
            editor.info.width, editor.info.height, ZoomPresets.label(canvas.viewport.zoom),
            workspace.tools.tool.localizedLabel)
        return (label, value)
    }

    /// Updates the canvas element; VoiceOver hears a value change.
    func updateCanvasAccessibility() {
        let d = canvasAccessibilityDescription
        guard canvas.accessibilityLabel() != d.label || (canvas.accessibilityValue() as? String) != d.value else { return }
        canvas.setAccessibilityLabel(d.label)
        canvas.setAccessibilityValue(d.value)
        NSAccessibility.post(element: canvas, notification: .valueChanged)
    }
}

extension PaintTool {
    /// The tool's name in the user's language.
    public var localizedLabel: String { L10n.tr(label, "Tool name (Tools panel, tool popup, tooltips)") }
}
