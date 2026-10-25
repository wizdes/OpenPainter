// The Edit menu's selection items (M29): Erase Selection (Delete), Fill
// Selection (Shift-Delete, the primary color), Invert Selection (a plain
// engine command in MenuSkeleton), Expand Selection…, Copy Selection and the
// Paste Selection submenu. Each item that changes the document sends one
// engine command, so it is one history step; Copy Selection changes nothing.
// An item is off when it would do nothing: Erase, Fill and Expand need a
// selection that covers a pixel (the engine's `enabled` rule), Copy needs a
// selection, and Paste needs selection text on the clipboard.
//
// paint.net (https://www.getpaint.net/doc/latest/EditMenu.html): Erase makes
// the pixels transparent and deselects; Fill keeps the selection; Copy
// Selection puts the outline (not pixels) on the clipboard as
// `{"polygonList": ["x,y,x,y,..."]}` JSON text, and Paste Selection reads it
// back with a combine mode from its submenu. Copy Selection is
// Ctrl+Alt+Shift+C and Paste Selection (Replace) Ctrl+Alt+Shift+V
// (KeyboardMouseCommands.html), so Option-Shift-Command-C and -V here.
// paint.net has no Expand item; Expand Selection… (a negative number shrinks)
// is OpenPainter's.
import AppKit

/// Edit menu selectors the document window controller implements (M29).
@MainActor @objc public protocol SelectionEditActions {
    func eraseSelection(_ sender: Any?)
    func fillSelection(_ sender: Any?)
    func copySelection(_ sender: Any?)
    /// The sender's tag is the combine mode's index in `ToolSettings.SelectionMode.allCases`.
    func pasteSelection(_ sender: Any?)
    func showExpandSelection(_ sender: Any?)
}

/// Where Copy Selection writes and Paste Selection reads. Tests swap in a
/// private pasteboard so they never touch the user's clipboard.
@MainActor
public enum SelectionClipboard {
    public static var pasteboard = NSPasteboard.general

    /// paint.net's Paste Selection submenu titles, in combine-mode order.
    /// paint.net 5.1's titles (EditMenu.html screenshot), in mode order (M55).
    public nonisolated static let pasteTitles = ["Replace", "Add (union)", "Exclude", "Intersect", "Invert (\"xor\")"]

    /// The clipboard text when it is selection text with at least one polygon.
    public static func selectionText() -> String? {
        guard let text = pasteboard.string(forType: .string),
              let data = text.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let list = json["polygonList"] as? [String], !list.isEmpty
        else { return nil }
        return text
    }
}

extension EditorWindowController: SelectionEditActions {
    /// Erase, Fill and Expand: the engine says whether the selection covers a pixel.
    func validateSelectionEdit(_ item: NSMenuItem) -> Bool {
        let target = FocusRouter.target(in: window)
        switch item.action {
        case #selector(eraseSelection(_:)):
            // Delete and Shift-Delete are text keys: a focused field keeps them.
            return target == .canvas && editor.isEnabled("selection.erase")
        case #selector(fillSelection(_:)):
            return target == .canvas && editor.isEnabled("selection.fill")
        case #selector(showExpandSelection(_:)):
            return target != .dialog && editor.isEnabled("selection.expand")
        case #selector(copySelection(_:)):
            return target != .dialog && editor.selection != nil
        case #selector(pasteSelection(_:)):
            return target != .dialog && SelectionClipboard.selectionText() != nil
        default:
            return true
        }
    }

    @objc public func eraseSelection(_ sender: Any?) { eraseSelection() }
    @objc public func fillSelection(_ sender: Any?) { fillSelection() }
    @objc public func copySelection(_ sender: Any?) { copySelectionOutline() }

    @objc public func pasteSelection(_ sender: Any?) {
        let tag = (sender as? NSMenuItem)?.tag ?? 0
        let modes = ToolSettings.SelectionMode.allCases
        pasteSelectionOutline(mode: modes[min(max(tag, 0), modes.count - 1)])
    }

    @objc public func showExpandSelection(_ sender: Any?) { presentExpandSelection() }

    /// Delete: one Erase Selection step, or a status message when nothing is selected.
    @discardableResult
    public func eraseSelection() -> UInt64? {
        guard editor.isEnabled("selection.erase") else {
            editor.report(editor.selection == nil ? "Erase Selection: nothing is selected"
                : "Erase Selection: the selection is empty")
            return nil
        }
        return editor.run("selection.erase")
    }

    /// Shift-Delete: one Fill Selection step with the primary color.
    @discardableResult
    public func fillSelection() -> UInt64? {
        guard editor.isEnabled("selection.fill") else {
            editor.report(editor.selection == nil ? "Fill Selection: nothing is selected"
                : "Fill Selection: the selection is empty")
            return nil
        }
        return editor.run("selection.fill", ["color": workspace.tools.primary.engineHex])
    }

    /// One Expand Selection (radius > 0) or Contract Selection (radius < 0) step.
    @discardableResult
    public func expandSelection(by radius: Int) -> UInt64? {
        guard radius != 0, editor.isEnabled("selection.expand") else { return nil }
        return editor.run("selection.expand", ["radius": min(max(radius, -1000), 1000)])
    }

    /// Writes the selection outline to the clipboard as paint.net's JSON text.
    /// False when nothing is selected or the engine cannot give the outline.
    @discardableResult
    public func copySelectionOutline() -> Bool {
        guard editor.selection != nil else { return false }
        do {
            let json = try editor.document.query("doc.selection_polygons")
            let data = try JSONSerialization.data(withJSONObject: json, options: [.prettyPrinted, .sortedKeys])
            guard let text = String(data: data, encoding: .utf8) else { return false }
            let pb = SelectionClipboard.pasteboard
            pb.clearContents()
            pb.setString(text, forType: .string)
            return true
        } catch {
            editor.report("could not copy the selection: \(error)")
            return false
        }
    }

    /// One Paste Selection step from the clipboard text in `mode`.
    @discardableResult
    public func pasteSelectionOutline(mode: ToolSettings.SelectionMode) -> UInt64? {
        guard let text = SelectionClipboard.selectionText() else {
            editor.report("Paste Selection: the clipboard has no selection")
            return nil
        }
        return editor.run("selection.paste", ["polygons": text, "combine": mode.rawValue])
    }

    /// Edit > Expand Selection…: a sheet that asks for a number of pixels.
    @discardableResult
    public func presentExpandSelection() -> ExpandSelectionDialog? {
        guard let window, window.attachedSheet == nil, editor.isEnabled("selection.expand") else { return nil }
        let d = ExpandSelectionDialog { [weak self] r in self?.expandSelection(by: r) }
        d.present(on: window)
        return d
    }
}

/// The Expand Selection sheet: one whole number of pixels, -1000 to 1000. A
/// positive number grows the selection, a negative one shrinks it; 0 or text
/// that is not a number keeps OK off.
@MainActor
public final class ExpandSelectionDialog: NSObject, NSTextFieldDelegate {
    public let alert = NSAlert()
    public let field = Labels.field("1", width: 80, height: 24)
    let apply: (Int) -> Void

    init(apply: @escaping (Int) -> Void) {
        self.apply = apply
        super.init()
        alert.messageText = "Expand Selection"
        alert.informativeText = "Grow the selection by this many pixels. A negative number shrinks it."
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        field.setAccessibilityLabel("Pixels")
        field.delegate = self
        let row = NSStackView(views: [field, Labels.label("px")])
        row.orientation = .horizontal
        row.spacing = 6
        row.frame = NSRect(x: 0, y: 0, width: 200, height: 24)
        alert.accessoryView = row
    }

    /// The typed radius, or nil when it is not a whole number in range or is 0.
    public var radius: Int? {
        guard let v = Int(field.stringValue.trimmingCharacters(in: .whitespaces)), v != 0, abs(v) <= 1000 else {
            return nil
        }
        return v
    }

    public var okButton: NSButton { alert.buttons[0] }

    public func controlTextDidChange(_ obj: Notification) {
        okButton.isEnabled = radius != nil
    }

    func present(on window: NSWindow) {
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { [self] response in
            if response == .alertFirstButtonReturn, let r = radius { apply(r) }
        }
    }

    /// Ends the sheet with OK (tests and Return).
    public func confirm() { okButton.performClick(nil) }
    /// Ends the sheet with Cancel.
    public func cancel() { alert.buttons[1].performClick(nil) }
}
