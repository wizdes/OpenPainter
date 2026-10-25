// Focus rules (mac/README.md, "Focus rules"): Cmd+A, Delete, Escape, Enter,
// Undo and Redo go to the canvas document unless a text field or a dialog has
// focus; then the field or dialog gets them.
import AppKit

public enum FocusTarget: Equatable, Sendable {
    /// The active document gets the command.
    case canvas
    /// A text field or text view is editing; it gets the command.
    case textField
    /// A sheet or modal dialog is up; the document gets nothing.
    case dialog
}

@MainActor
public enum FocusRouter {
    /// Who should get a key command typed while `window` is key.
    public static func target(in window: NSWindow?) -> FocusTarget {
        guard let window else { return .canvas }
        if window.attachedSheet != nil || window.isSheet || window.isModalPanel { return .dialog }
        if NSApp.modalWindow != nil, NSApp.modalWindow !== window { return .dialog }
        if isText(window.firstResponder) { return .textField }
        return .canvas
    }

    static func isText(_ responder: NSResponder?) -> Bool {
        if let tv = responder as? NSTextView { return tv.isEditable || tv.isFieldEditor }
        if let tf = responder as? NSTextField { return tf.isEditable }
        return false
    }
}

/// The document window. It routes the focus-rule commands: with the canvas (or
/// any non-text control) focused they go to the editor; with a text field
/// focused they go to the field; with a sheet up the document gets nothing.
public final class EditorWindow: NSWindow {
    weak var editorController: EditorWindowController?

    private var routesToCanvas: Bool { FocusRouter.target(in: self) == .canvas }

    @objc public func undo(_ sender: Any?) {
        if routesToCanvas {
            editorController?.undo()
        } else if FocusRouter.target(in: self) == .textField {
            undoManager?.undo()
        }
    }

    @objc public func redo(_ sender: Any?) {
        if routesToCanvas {
            editorController?.redo()
        } else if FocusRouter.target(in: self) == .textField {
            undoManager?.redo()
        }
    }

    @objc public override func selectAll(_ sender: Any?) {
        if routesToCanvas { editorController?.selectAllPixels() }
    }

    @objc public func delete(_ sender: Any?) {
        if routesToCanvas { editorController?.canvasKey(.delete) }
    }

    public override func cancelOperation(_ sender: Any?) {
        if routesToCanvas { editorController?.canvasKey(.escape) } else { super.cancelOperation(sender) }
    }

    public override func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        switch item.action {
        case #selector(undo(_:)):
            if FocusRouter.target(in: self) == .textField { return undoManager?.canUndo ?? false }
            return routesToCanvas && (editorController?.canUndo ?? false)
        case #selector(redo(_:)):
            if FocusRouter.target(in: self) == .textField { return undoManager?.canRedo ?? false }
            return routesToCanvas && (editorController?.canRedo ?? false)
        case #selector(selectAll(_:)), #selector(delete(_:)):
            return routesToCanvas && editorController != nil
        default:
            return super.validateUserInterfaceItem(item)
        }
    }
}
