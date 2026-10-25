// The command buttons at the start of the tool-options row (no NSToolbar):
// New, Open, Save, Undo, Redo, Crop to Selection, Magic Wand, then "More",
// whose menu holds the rest of the old toolbar. Every button and menu item
// uses the same action and the same enabled rule as its menu command.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class CommandBarTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    static let order = ["New", "Open", "Save", "Undo", "Redo", "Crop to Selection", "Magic Wand", "More commands"]
    static let moreTitles = ["Print", "Cut", "Copy", "Paste", "Deselect", "Pixel Grid", "Rulers"]
        + PanelKind.allCases.map(\.title)

    /// Records each command the bar sends, then lets it run.
    final class Spy {
        var sent: [(Selector, AnyObject)] = []
    }

    func spy(_ wc: EditorWindowController, run: Bool = true) -> Spy {
        let s = Spy()
        let real = wc.sendCommand
        wc.sendCommand = { action, target, sender in
            s.sent.append((action, target))
            if run { real(action, target, sender) }
        }
        return s
    }

    func testWindowHasNoToolbarAndKeepsItsTitleBar() throws {
        let wc = try fx.open()
        let window = try XCTUnwrap(wc.window)
        XCTAssertNil(window.toolbar, "the separate toolbar row is gone")
        XCTAssertEqual(window.title, "plasma_alpha_512x384.png")
        XCTAssertEqual(window.titleVisibility, .visible)
        XCTAssertTrue(window.styleMask.contains(.titled))
        XCTAssertTrue(window.styleMask.contains(.resizable))
        for b in [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton] {
            XCTAssertNotNil(window.standardWindowButton(b), "traffic light \(b.rawValue)")
        }
        XCTAssertTrue(window.collectionBehavior.contains(.fullScreenPrimary), "full screen stays available")
        // The Edited state still reaches the title bar.
        wc.editor.run("image.flip_horizontal")
        XCTAssertTrue(waitUntil { window.isDocumentEdited })
    }

    func testSevenButtonsThenMoreLeadTheOptionsRowInOrder() throws {
        let wc = try fx.open()
        XCTAssertEqual(wc.commandBarButtons.map { $0.accessibilityLabel() ?? "" }, Self.order)
        XCTAssertEqual(Array(wc.commandBarButtons.prefix(7)), Self.keys.map { wc.commandButtons[$0]! })
        XCTAssertTrue(wc.commandBarButtons.last === wc.moreButton)
        XCTAssertEqual(wc.commandButtons["new"]?.toolTip, "New (⌘N)")
        XCTAssertEqual(wc.commandButtons["open"]?.toolTip, "Open (⌘O)")
        XCTAssertEqual(wc.commandButtons["save"]?.toolTip, "Save (⌘S)")
        XCTAssertEqual(wc.commandButtons["undo"]?.toolTip, "Undo (⌘Z)")
        XCTAssertEqual(wc.commandButtons["redo"]?.toolTip, "Redo (⇧⌘Z)")
        XCTAssertEqual(wc.commandButtons["crop"]?.toolTip, "Crop to Selection (⇧⌘X)")
        XCTAssertEqual(wc.commandButtons["wand"]?.toolTip, "Magic Wand (S)")
        XCTAssertEqual(wc.moreButton.toolTip, "More commands")
        // Left to right, all before the tool picker, inside the options row.
        let bar = wc.optionsBar
        let xs = wc.commandBarButtons.map { bar.convert($0.bounds, from: $0).minX }
        XCTAssertEqual(xs, xs.sorted())
        let picker = bar.convert(bar.picker.bounds, from: bar.picker)
        XCTAssertLessThan(bar.convert(wc.moreButton.bounds, from: wc.moreButton).maxX, picker.minX)
    }

    static let keys = ["new", "open", "save", "undo", "redo", "crop", "wand"]

    func testButtonsFollowTheMenuEnabledRules() throws {
        let wc = try fx.open()
        let editor = wc.editor
        wc.refreshCommandBar()
        XCTAssertTrue(wc.commandButtons["new"]!.isEnabled)
        XCTAssertTrue(wc.commandButtons["open"]!.isEnabled)
        XCTAssertFalse(wc.commandButtons["save"]!.isEnabled, "no document: the menu's Save is off too")
        XCTAssertFalse(wc.commandButtons["undo"]!.isEnabled, "nothing to undo")
        XCTAssertFalse(wc.commandButtons["redo"]!.isEnabled, "nothing to redo")
        XCTAssertFalse(wc.commandButtons["crop"]!.isEnabled, "no selection")
        XCTAssertTrue(wc.commandButtons["wand"]!.isEnabled)
        XCTAssertTrue(wc.moreButton.isEnabled)
        editor.run("selection.rectangle", ["x": 10.0, "y": 20.0, "width": 30.0, "height": 40.0])
        XCTAssertTrue(waitUntil { editor.selection != nil && wc.commandButtons["crop"]!.isEnabled })
        XCTAssertTrue(waitUntil { wc.commandButtons["undo"]!.isEnabled }, "the selection is an undo step")
        // A sheet makes the document busy: Crop and Undo turn off as in the menu.
        let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 60), styleMask: [.titled],
                             backing: .buffered, defer: false)
        wc.window!.beginSheet(sheet)
        XCTAssertTrue(waitUntil { wc.window!.attachedSheet != nil })
        wc.refreshCommandBar()
        XCTAssertFalse(wc.commandButtons["crop"]!.isEnabled)
        XCTAssertFalse(wc.commandButtons["undo"]!.isEnabled)
        wc.window!.endSheet(sheet)
        XCTAssertTrue(waitUntil { wc.window!.attachedSheet == nil })
        wc.refreshCommandBar()
        XCTAssertTrue(wc.commandButtons["crop"]!.isEnabled)
    }

    func testEachButtonFiresTheMenuCommand() throws {
        let wc = try fx.open()
        let editor = wc.editor
        let original = wc.sendCommand
        // New and Open go to the document controller, as File > New and Open do.
        let s = spy(wc, run: false)
        wc.commandButtons["new"]!.performClick(nil)
        wc.commandButtons["open"]!.performClick(nil)
        XCTAssertEqual(s.sent.map(\.0), [#selector(NSDocumentController.newDocument(_:)),
                                         #selector(NSDocumentController.openDocument(_:))])
        XCTAssertTrue(s.sent.allSatisfy { $0.1 === NSDocumentController.shared })
        // Save goes to the window's document, as File > Save does.
        let doc = NSDocument()
        doc.addWindowController(wc)
        defer { doc.removeWindowController(wc) }
        wc.refreshCommandBar()
        XCTAssertTrue(wc.commandButtons["save"]!.isEnabled)
        wc.commandButtons["save"]!.performClick(nil)
        XCTAssertEqual(s.sent.last?.0, #selector(NSDocument.save(_:)))
        XCTAssertTrue(s.sent.last?.1 === doc)
        // Crop, Undo and Redo run for real.
        wc.sendCommand = original
        let live = spy(wc)
        editor.run("selection.rectangle", ["x": 10.0, "y": 20.0, "width": 30.0, "height": 40.0])
        XCTAssertTrue(waitUntil { wc.commandButtons["crop"]!.isEnabled })
        wc.commandButtons["crop"]!.performClick(nil)
        XCTAssertTrue(waitUntil { editor.info.width == 30 && editor.info.height == 40 }, "cropped")
        XCTAssertEqual(live.sent.last?.0, #selector(EditorMenuActions.runCommand(_:)))
        wc.refreshCommandBar()
        XCTAssertTrue(wc.commandButtons["undo"]!.isEnabled)
        wc.commandButtons["undo"]!.performClick(nil)
        XCTAssertTrue(waitUntil { editor.info.width == 512 }, "undo brings the size back")
        XCTAssertEqual(live.sent.last?.0, #selector(StandardEditActions.undo(_:)))
        XCTAssertTrue(live.sent.last?.1 === wc.window, "Undo goes through the window's focus rules")
        wc.refreshCommandBar()
        XCTAssertTrue(wc.commandButtons["redo"]!.isEnabled)
        wc.commandButtons["redo"]!.performClick(nil)
        XCTAssertTrue(waitUntil { editor.info.width == 30 }, "redo crops again")
        // Magic Wand picks the tool, as its Tools panel button does, and shows it on.
        let wand = try XCTUnwrap(wc.commandButtons["wand"] as? ToggleButton)
        XCTAssertNotEqual(fx.workspace.tools.tool, .magicWand)
        XCTAssertFalse(wand.isOn)
        wand.performClick(nil)
        XCTAssertEqual(fx.workspace.tools.tool, .magicWand)
        XCTAssertTrue(wand.isOn)
        wand.performClick(nil)
        XCTAssertEqual(fx.workspace.tools.tool, .magicWand, "a second click keeps the tool")
        XCTAssertTrue(wand.isOn)
        fx.workspace.tools.tool = .paintbrush
        XCTAssertFalse(wand.isOn, "another tool turns it off")
    }

    func testMoreMenuHoldsTheRestAndEachItemWorks() throws {
        let wc = try fx.open()
        let editor = wc.editor
        let panels = fx.workspace.panels
        func menu() -> NSMenu {
            let m = wc.moreMenu()
            m.update()
            return m
        }
        func item(_ m: NSMenu, _ title: String) -> NSMenuItem { m.items.first { $0.title == title }! }
        let m = menu()
        XCTAssertEqual(m.items.filter { !$0.isSeparatorItem }.map(\.title), Self.moreTitles)
        // Shortcuts as in the main menu; the panels show F5 to F8.
        XCTAssertEqual(item(m, "Cut").keyEquivalent, "x")
        XCTAssertEqual(item(m, "Rulers").keyEquivalentModifierMask, [.command, .option])
        for k in PanelKind.allCases {
            XCTAssertEqual(item(m, k.title).keyEquivalent, functionKey(k.functionKey), k.title)
            XCTAssertEqual(item(m, k.title).toolTip, "\(k.title) (F\(k.functionKey) or ⌥⌘\(k.digit))")
        }
        // Enabled rules: Print is not built; Deselect needs a selection; Copy has pixels.
        XCTAssertFalse(item(m, "Print").isEnabled)
        XCTAssertFalse(item(m, "Deselect").isEnabled)
        XCTAssertTrue(item(m, "Copy").isEnabled)
        XCTAssertTrue(item(m, "Pixel Grid").isEnabled)
        XCTAssertTrue(item(m, "Rulers").isEnabled)
        // Cut, Copy and Paste reach the Edit menu's clipboard actions (a private pasteboard).
        let board = NSPasteboard(name: NSPasteboard.Name("OpenPainterTests.\(UUID().uuidString)"))
        let savedBoard = ImageClipboard.pasteboard
        ImageClipboard.pasteboard = board
        defer {
            ImageClipboard.pasteboard = savedBoard
            board.releaseGlobally()
        }
        for t in ["Cut", "Copy", "Paste"] {
            XCTAssertTrue(item(m, t).target === wc, t)
            XCTAssertEqual(item(m, t).action, [#selector(NSText.cut(_:)), #selector(NSText.copy(_:)),
                                               #selector(NSText.paste(_:))][["Cut", "Copy", "Paste"].firstIndex(of: t)!])
        }
        var mm = menu()
        XCTAssertFalse(item(mm, "Paste").isEnabled, "nothing on the pasteboard")
        XCTAssertFalse(item(mm, "Cut").isEnabled, "no selection to cut")
        mm.performActionForItem(at: mm.index(of: item(mm, "Copy")))
        XCTAssertTrue(waitUntil { ImageClipboard.hasImage }, "Copy put the layer on the pasteboard")
        mm = menu()
        XCTAssertTrue(item(mm, "Paste").isEnabled)
        mm.performActionForItem(at: mm.index(of: item(mm, "Paste")))
        // The paste opens a live Move Selected Pixels session, as Edit > Paste does.
        XCTAssertTrue(waitUntil { wc.moveTool.active }, "Paste opened a paste session: \(editor.message ?? "no message")")
        XCTAssertEqual(fx.workspace.tools.tool, .moveSelectedPixels)
        _ = wc.moveTool.finish()
        editor.run("selection.rectangle", ["x": 10.0, "y": 20.0, "width": 30.0, "height": 40.0])
        XCTAssertTrue(waitUntil { editor.selection?.width == 30 })
        let before = editor.info.historyLabels.count
        mm = menu()
        XCTAssertTrue(item(mm, "Cut").isEnabled)
        mm.performActionForItem(at: mm.index(of: item(mm, "Cut")))
        XCTAssertTrue(waitUntil { editor.info.historyLabels.count > before }, "Cut erased the selection")
        // Deselect.
        editor.run("selection.select_all")
        XCTAssertTrue(waitUntil { editor.selection != nil })
        mm = menu()
        XCTAssertTrue(item(mm, "Deselect").isEnabled)
        mm.performActionForItem(at: mm.index(of: item(mm, "Deselect")))
        XCTAssertTrue(waitUntil { editor.selection == nil })
        // Pixel Grid and Rulers toggle and show check marks.
        mm = menu()
        XCTAssertEqual(item(mm, "Pixel Grid").state, .off)
        XCTAssertEqual(item(mm, "Rulers").state, .off)
        mm.performActionForItem(at: mm.index(of: item(mm, "Pixel Grid")))
        mm.performActionForItem(at: mm.index(of: item(mm, "Rulers")))
        XCTAssertTrue(wc.canvas.pixelGrid)
        XCTAssertTrue(wc.container.rulersVisible)
        mm = menu()
        XCTAssertEqual(item(mm, "Pixel Grid").state, .on)
        XCTAssertEqual(item(mm, "Rulers").state, .on)
        // Panel toggles hide and show the panels, with check marks.
        for k in PanelKind.allCases {
            mm = menu()
            XCTAssertEqual(item(mm, k.title).state, .on, "\(k) shown")
            mm.performActionForItem(at: mm.index(of: item(mm, k.title)))
            XCTAssertFalse(panels.isShown(k), "\(k) hidden from the More menu")
            XCTAssertFalse(panels.panels[k]!.isVisible)
            mm = menu()
            XCTAssertEqual(item(mm, k.title).state, .off)
            mm.performActionForItem(at: mm.index(of: item(mm, k.title)))
            XCTAssertTrue(panels.isShown(k), "\(k) shown again")
        }
    }

    /// Edit > Paste (and More > Paste) call `paste(_:)`, which passes no
    /// callback. The paste must still start (it was skipped before: the call
    /// sat inside `done?(...)`).
    func testEditPasteWithNoCallbackStartsThePaste() throws {
        let wc = try fx.open()
        let board = NSPasteboard(name: NSPasteboard.Name("OpenPainterTests.\(UUID().uuidString)"))
        let savedBoard = ImageClipboard.pasteboard
        ImageClipboard.pasteboard = board
        defer {
            ImageClipboard.pasteboard = savedBoard
            board.releaseGlobally()
        }
        XCTAssertTrue(wc.copyPixels(merged: false))
        wc.paste(nil)
        XCTAssertTrue(wc.moveTool.active, "a paste session opened")
        XCTAssertEqual(fx.workspace.tools.tool, .moveSelectedPixels)
    }

    func testButtonsStayVisibleAtTheMinimumWindowWidth() throws {
        let wc = try fx.open()
        let window = try XCTUnwrap(wc.window)
        window.setContentSize(window.contentMinSize)
        window.layoutIfNeeded()
        wc.optionsBar.layoutSubtreeIfNeeded()
        XCTAssertEqual(window.contentView!.bounds.width, min(1100, NSScreen.main?.visibleFrame.width ?? 1100))
        let bar = wc.optionsBar
        let commands = try XCTUnwrap(bar.commandBar)
        /// Each button sits in the fixed command area, which the row holds whole;
        /// the tool options scroll in their own view after it.
        func check(_ what: String) {
            XCTAssertGreaterThanOrEqual(commands.frame.minX, 0, what)
            XCTAssertLessThanOrEqual(commands.frame.maxX, bar.bounds.width, what)
            for b in wc.commandBarButtons {
                XCTAssertTrue(b.isDescendant(of: commands), what)
                XCTAssertFalse(b.isHiddenOrHasHiddenAncestor)
                let r = commands.convert(b.alignmentRect(forFrame: b.frame), from: b.superview)
                XCTAssertGreaterThanOrEqual(r.minX, 0, "\(b.accessibilityLabel() ?? "") \(what)")
                XCTAssertLessThanOrEqual(r.maxX, commands.bounds.width, "\(b.accessibilityLabel() ?? "") \(what)")
                XCTAssertEqual(r.width, 28, accuracy: 0.5)
            }
            let options = try! XCTUnwrap(bar.picker.enclosingScrollView)
            XCTAssertGreaterThanOrEqual(options.frame.minX, commands.frame.maxX, "the tool options start after the commands \(what)")
        }
        for tool in [PaintTool.paintbrush, .magicWand, .text] {
            fx.workspace.tools.tool = tool
            bar.layoutSubtreeIfNeeded()
            check("with \(tool) at \(bar.bounds.width) pt")
        }
        // Narrower than the minimum (a test can force it): the commands still
        // fit; only the tool options get less room.
        bar.setFrameSize(NSSize(width: 360, height: bar.frame.height))
        bar.layoutSubtreeIfNeeded()
        check("at 360 pt")
    }
}
