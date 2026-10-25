// Focus rules: Cmd+A, Delete, Escape, Enter, Undo and Redo go to the canvas
// document unless a text field or a dialog (sheet) has focus.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class FocusTests: XCTestCase {
    var fx: ShellFixture!
    var wc: EditorWindowController!
    var window: NSWindow { wc.window! }
    var editor: Editor { wc.editor }

    override func setUp() async throws {
        fx = try ShellFixture()
        wc = try fx.open()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    /// What a menu item with this action does: the responder chain from the
    /// window's first responder (the same walk NSApp.sendAction makes).
    @discardableResult
    func send(_ action: Selector, in w: NSWindow? = nil) -> Bool {
        let w = w ?? window
        return (w.firstResponder ?? w).tryToPerform(action, with: nil)
    }

    func key(_ chars: String, _ code: UInt16, in w: NSWindow? = nil) {
        let w = w ?? window
        w.firstResponder?.keyDown(with: keyEvent(chars, keyCode: code, window: w))
    }

    func testCanvasContextSendsCommandsToTheDocument() throws {
        window.makeFirstResponder(wc.canvas)
        XCTAssertEqual(FocusRouter.target(in: window), .canvas)
        send(#selector(NSResponder.selectAll(_:)))
        XCTAssertTrue(waitUntil { self.editor.selection != nil }, "Cmd+A selects all pixels")
        XCTAssertEqual(editor.selection, CGRect(x: 0, y: 0, width: 512, height: 384))
        key("\u{1b}", 53)
        XCTAssertTrue(waitUntil { self.editor.selection == nil }, "Escape deselects")
        send(#selector(EditorWindow.undo(_:)))
        XCTAssertTrue(waitUntil { self.editor.selection != nil }, "Undo brings the selection back")
        send(#selector(EditorWindow.redo(_:)))
        XCTAssertTrue(waitUntil { self.editor.selection == nil }, "Redo removes it again")
        key("\u{7f}", 51)
        XCTAssertEqual(editor.message, "Erase Selection: nothing is selected", "Delete reaches the document")
        key("\r", 36)
        XCTAssertNil(editor.message, "Enter reaches the document")
        // A non-text control with focus (an image-strip item) still routes to the document.
        let item = try XCTUnwrap(wc.strip.items.first)
        window.makeFirstResponder(item)
        XCTAssertEqual(FocusRouter.target(in: window), .canvas)
        send(#selector(NSResponder.selectAll(_:)))
        XCTAssertTrue(waitUntil { self.editor.selection != nil })
    }

    func testTextFieldContextKeepsCommandsInTheField() throws {
        let field = wc.optionsBar.widthField
        XCTAssertTrue(window.makeFirstResponder(field))
        let editorView = try XCTUnwrap(window.fieldEditor(false, for: field) as? NSTextView)
        XCTAssertTrue(window.firstResponder === editorView)
        XCTAssertEqual(FocusRouter.target(in: window), .textField)
        editorView.insertText("123", replacementRange: NSRange(location: 0, length: (editorView.string as NSString).length))
        let history = editor.info.historyLabels.count
        send(#selector(NSResponder.selectAll(_:)))
        XCTAssertEqual(editorView.selectedRange(), NSRange(location: 0, length: 3), "Cmd+A selects the field's text")
        key("\u{7f}", 51)
        XCTAssertEqual(editorView.string, "", "Delete edits the field")
        key("\u{1b}", 53)
        send(#selector(EditorWindow.undo(_:)))
        send(#selector(EditorWindow.redo(_:)))
        // Nothing reached the document.
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        XCTAssertNil(editor.selection)
        XCTAssertEqual(editor.info.historyLabels.count, history)
        XCTAssertNil(editor.message)
    }

    func testSheetContextGivesTheDocumentNothing() throws {
        let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 120), styleMask: [.titled],
                             backing: .buffered, defer: false)
        sheet.isReleasedWhenClosed = false
        let field = NSTextField(string: "abc")
        field.frame = NSRect(x: 20, y: 40, width: 200, height: 24)
        sheet.contentView?.addSubview(field)
        window.beginSheet(sheet)
        XCTAssertTrue(waitUntil { self.window.attachedSheet === sheet })
        XCTAssertEqual(FocusRouter.target(in: window), .dialog)
        // Commands sent to the document window do nothing while the sheet is up.
        send(#selector(NSResponder.selectAll(_:)))
        send(#selector(EditorWindow.undo(_:)))
        let undoItem = NSMenuItem(title: "Undo", action: #selector(EditorWindow.undo(_:)), keyEquivalent: "z")
        XCTAssertFalse((window as! EditorWindow).validateUserInterfaceItem(undoItem))
        // In the sheet, the field gets them.
        XCTAssertTrue(sheet.makeFirstResponder(field))
        XCTAssertEqual(FocusRouter.target(in: sheet), .dialog)
        send(#selector(NSResponder.selectAll(_:)), in: sheet)
        let fieldEditor = try XCTUnwrap(sheet.fieldEditor(false, for: field) as? NSTextView)
        XCTAssertEqual(fieldEditor.selectedRange(), NSRange(location: 0, length: 3))
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        XCTAssertNil(editor.selection, "the document got nothing")
        window.endSheet(sheet)
        XCTAssertTrue(waitUntil { self.window.attachedSheet == nil })
        sheet.close()
        window.makeFirstResponder(wc.canvas)
        XCTAssertEqual(FocusRouter.target(in: window), .canvas)
    }

    func testPanelWithoutFieldFocusForwardsToTheDocument() throws {
        let panel = fx.workspace.panels.panels[.history]!
        XCTAssertEqual(FocusRouter.target(in: panel), .canvas)
        let undoItem = NSMenuItem(title: "Undo", action: #selector(EditorWindow.undo(_:)), keyEquivalent: "z")
        XCTAssertFalse(panel.validateUserInterfaceItem(undoItem), "nothing to undo yet")
        panel.selectAll(nil)
        XCTAssertTrue(waitUntil { self.editor.selection != nil }, "Cmd+A in a panel selects pixels")
        XCTAssertTrue(panel.validateUserInterfaceItem(undoItem), "Undo in a panel follows the document")
        panel.undo(nil)
        XCTAssertTrue(waitUntil { self.editor.selection == nil }, "Cmd+Z in a panel undoes in the document")
        // The Colors hex field takes them while it edits.
        let colors = fx.workspace.panels.panels[.colors]!
        let hex = fx.workspace.colorsView.hexField
        XCTAssertTrue(colors.makeFirstResponder(hex))
        XCTAssertEqual(FocusRouter.target(in: colors), .textField)
    }
}
