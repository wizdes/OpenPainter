// M59 keyboard and accessibility:
// - docs/shortcuts.md is generated from the menu catalog and
//   KeyboardShortcuts.canvas; no two keys clash and none takes a macOS key.
// - The canvas keys (Shift-letter cycling, brackets, X, C, Control-Tab).
// - Focus: the Next Panel cycle and the key view loop of every panel and dialog.
// - A keyboard-only end-to-end run: open, Select All, Brightness / Contrast
//   from its dialog with Tab and Return, Undo, Redo, Save.
// - VoiceOver: every control has a label; the walkthrough transcript
//   (docs/accessibility-walkthrough.md) is read from the NSAccessibility tree
//   and written by the test, with actions performed through it.
// - Strings: every string the windows show is in Localizable.xcstrings, and a
//   pseudo-localized run renders without clipped text (PNGs when
//   TEST_RUNNER_M59_PNG_DIR is set).
// In-process XCTest (XCUITest is not available on this Mac).
import AppKit
@testable import OpenPainterKit
import XCTest

let repoRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    .appendingPathComponent("../../..").standardizedFileURL

// MARK: Accessibility tree

/// One element of the NSAccessibility tree.
struct AXNode {
    let depth: Int
    let role: String
    let subrole: String
    let label: String
    let title: String
    let value: String
    let help: String
    let enabled: Bool
    let element: NSAccessibilityProtocol

    /// What VoiceOver reads first: the label, else the title.
    var name: String { label.isEmpty ? title : label }

    var line: String {
        var s = String(repeating: "  ", count: depth) + role.replacingOccurrences(of: "AX", with: "")
        if !subrole.isEmpty { s += " (\(subrole.replacingOccurrences(of: "AX", with: "")))" }
        if !name.isEmpty { s += " \"\(name)\"" }
        if !value.isEmpty { s += " = \(value)" }
        if !enabled, controlRoles.contains(NSAccessibility.Role(rawValue: role)) { s += " [off]" }
        if !help.isEmpty { s += " — help: \(help)" }
        return s
    }
}

/// Roles that a person operates: each needs a name VoiceOver can read.
let controlRoles: Set<NSAccessibility.Role> = [
    .button, .checkBox, .radioButton, .popUpButton, .menuButton, .slider, .textField, .comboBox, .colorWell,
    .incrementor, .disclosureTriangle, .textArea, .image, .list, .heading,
]

@MainActor
func axString(_ v: Any?) -> String {
    switch v {
    case nil: return ""
    case let s as String: return s
    case let n as NSNumber: return n.stringValue
    case let a as NSAttributedString: return a.string
    default: return ""
    }
}

/// Walks the accessibility tree from `root` (a window or view), the way
/// VoiceOver sees it: `accessibilityChildren()` gives the unignored children.
@MainActor
func axTree(_ root: NSAccessibilityProtocol, depth: Int = 0, maxDepth: Int = 30) -> [AXNode] {
    var out: [AXNode] = []
    func visit(_ e: NSAccessibilityProtocol, _ d: Int) {
        guard d <= maxDepth else { return }
        if let v = e as? NSView, v.isHiddenOrHasHiddenAncestor { return }
        let role = e.accessibilityRole()?.rawValue ?? ""
        let node = AXNode(
            depth: d, role: role, subrole: e.accessibilitySubrole()?.rawValue ?? "",
            label: e.accessibilityLabel() ?? "", title: e.accessibilityTitle() ?? "",
            value: axString(e.accessibilityValue()),
            help: e.accessibilityHelp() ?? "", enabled: e.isAccessibilityEnabled(), element: e)
        out.append(node)
        // Menus inside popups are not part of the window walk.
        if role == NSAccessibility.Role.popUpButton.rawValue || role == NSAccessibility.Role.menuButton.rawValue { return }
        for c in e.accessibilityChildren() ?? [] {
            if let c = c as? NSAccessibilityProtocol { visit(c, d + 1) }
        }
    }
    visit(root, depth)
    return out
}

@MainActor
final class KeyboardAccessibilityTests: XCTestCase {
    var fixture: ShellFixture?
    var fx: ShellFixture {
        if let fixture { return fixture }
        let f = try! ShellFixture()
        fixture = f
        return f
    }

    var pngDir: URL? {
        ProcessInfo.processInfo.environment["M59_PNG_DIR"].map { URL(fileURLWithPath: $0) }
    }

    override func setUp() async throws {
        KeyboardAccess.override = true
        EffectDialogController.lastEffect = nil
        EffectDialogController.lastUsed = [:]
    }

    override func tearDown() async throws {
        L10n.pseudo = false
        L10n.recording = false
        KeyboardAccess.override = nil
        if let fixture {
            for wc in fixture.controllers { EffectDialogController.current(for: wc.editor)?.cancel(nil) }
            fixture.tearDown()
        }
        fixture = nil
    }

    func menu() -> NSMenu { MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: fx.workspace.menuTarget) }

    // MARK: Keys

    func key(_ chars: String, _ code: UInt16, _ mods: NSEvent.ModifierFlags = [], to view: NSView) {
        view.keyDown(with: keyEvent(chars, keyCode: code, modifiers: mods, window: view.window))
    }

    /// The menu item for a key equivalent, the way NSMenu matches it.
    func menuItem(_ menu: NSMenu, key: String, _ mods: NSEvent.ModifierFlags) -> NSMenuItem? {
        for item in menu.items {
            if let sub = item.submenu, let hit = menuItem(sub, key: key, mods) { return hit }
            if item.keyEquivalent == key,
               item.keyEquivalentModifierMask.intersection(.deviceIndependentFlagsMask) == mods { return item }
        }
        return nil
    }

    /// Presses a menu key: finds its item, checks it is on, and sends its
    /// action as NSApp.sendAction does: to the item's target, else up the
    /// responder chain from the window's first responder, then to the
    /// window's document. (The test process is not the active app, so
    /// NSApp has no key window to send to.)
    @discardableResult
    func pressMenuKey(_ menu: NSMenu, _ key: String, _ mods: NSEvent.ModifierFlags, in window: NSWindow,
                      file: StaticString = #filePath, line: UInt = #line) -> NSMenuItem? {
        guard let item = menuItem(menu, key: key, mods), let action = item.action else {
            XCTFail("no menu item for \(mods) \(key)", file: file, line: line)
            return nil
        }
        if let t = item.target {
            if let v = t as? NSMenuItemValidation {
                XCTAssertTrue(v.validateMenuItem(item), "\(item.title) is on", file: file, line: line)
            }
            NSApp.sendAction(action, to: t, from: item)
            return item
        }
        // The validation AppKit would ask: the document window validates the
        // focus-rule commands (FocusRouter.swift).
        if let w = window as? EditorWindow, w.responds(to: action) {
            XCTAssertTrue(w.validateUserInterfaceItem(item), "\(item.title) is on", file: file, line: line)
        }
        if (window.firstResponder ?? window).tryToPerform(action, with: item) { return item }
        if let doc = window.windowController?.document as? NSObject, doc.responds(to: action) {
            doc.perform(action, with: item)
            return item
        }
        XCTFail("\(item.title): nothing in the responder chain takes \(action)", file: file, line: line)
        return nil
    }

    // MARK: Shortcut table

    /// The keys bound outside the menus, in menu notation, by what they do.
    var canvasBindings: [(key: String, action: String)] {
        var out: [(String, String)] = []
        for letter in KeyboardShortcuts.uniqueToolKeys {
            let L = String(letter).uppercased()
            out.append((L, "tool \(L)"))
            out.append(("⇧" + L, "tool \(L) backwards"))
        }
        out += [("[", "width -1"), ("]", "width +1"), ("⌘[", "width -5"), ("⌘]", "width +5"), ("X", "swap"),
                ("C", "active slot"), ("⌃Tab", "next image"), ("⌃⇧Tab", "previous image")]
        return out
    }

    func testShortcutTableIsGeneratedAndNoKeyClashes() throws {
        let catalog = try MenuCatalog.load()
        let live = liveItems(menu())
        // 1. No two bindings share a key: menu items with each other and with
        // the canvas keys. paint.net's shared keys are the tool letters, which
        // cycle, and they are one binding each here.
        var owner: [String: String] = [:]
        for l in live where !l.key.isEmpty && !catalog.shared.contains(l.key) {
            XCTAssertNil(owner[l.key], "\(l.path) and \(owner[l.key] ?? "") share \(l.key)")
            owner[l.key] = l.path
        }
        for b in canvasBindings {
            XCTAssertNil(owner[b.key], "canvas \(b.action) and \(owner[b.key] ?? "") share \(b.key)")
            owner[b.key] = "canvas: \(b.action)"
        }
        // 2. Nothing takes a macOS system key, except the standard app items
        // (Hide, Quit, Minimize, Close, Settings) that are the app's own.
        var clashes: [String] = []
        for s in KeyboardShortcuts.system {
            guard let who = owner[s.key] else { continue }
            if s.standardAppItem { continue }
            clashes.append("\(s.key) \(s.what): \(who)")
        }
        XCTAssertEqual(clashes, [], "keys that macOS uses")
        // 3. Every catalog item with a key is in the table, and the table is
        // the checked-in file.
        let doc = shortcutsMarkdown(catalog)
        for i in catalog.items where !(i["key"] ?? "").isEmpty && !(i["mac"] ?? "").isEmpty {
            XCTAssertTrue(doc.contains("| \(i["mac"]!.replacingOccurrences(of: "|", with: "\\|")) |"), i["mac"]!)
        }
        let url = repoRoot.appendingPathComponent("docs/shortcuts.md")
        try doc.write(to: url, atomically: true, encoding: .utf8)
        XCTAssertTrue(doc.contains("| Next Panel |") || doc.contains("Window > Next Panel"))
    }

    func shortcutsMarkdown(_ catalog: MenuCatalog) -> String {
        func cell(_ s: String) -> String { s.isEmpty ? "–" : s.replacingOccurrences(of: "|", with: "\\|") }
        var md = """
        # Keyboard shortcuts

        Generated by `KeyboardAccessibilityTests.testShortcutTableIsGeneratedAndNoKeyClashes` from
        `tests/catalog/paintnet-5.1-menus.toml` (the menus) and `KeyboardShortcuts.canvas` in
        `mac/OpenPainterKit/Sources/Shell/KeyboardShortcuts.swift` (the other keys). Do not edit by hand.

        The rule: paint.net's Ctrl becomes ⌘ and Alt becomes ⌥. Where that would take a standard Mac key,
        the Mac key wins and the note says so. Sources: paint.net's Keyboard & Mouse Commands page
        (https://www.getpaint.net/doc/latest/KeyboardMouseCommands.html) and Apple's Mac keyboard shortcuts
        (https://support.apple.com/en-us/102650).

        ## Menu commands

        | Menu item | paint.net (Windows) | OpenPainter (Mac) | Status | Note |
        | --- | --- | --- | --- | --- |

        """
        for i in catalog.items {
            let win = i["win"] ?? "", key = i["key"] ?? "", mac = i["mac"] ?? ""
            guard !win.isEmpty || !key.isEmpty else { continue }
            let name = mac.isEmpty ? (i["pdn"] ?? "") : mac
            let note = i["note"] ?? i["reason"] ?? ""
            md += "| \(cell(name)) | \(cell(win)) | \(cell(key)) | \(cell(i["status"] ?? "")) | \(cell(note)) |\n"
        }
        md += """

        Window > Tools, History, Layers and Colors also answer ⌥⌘1 to ⌥⌘4 (hidden menu items).

        ## Canvas, panel and dialog keys

        These work while the canvas (or the named place) has keyboard focus; they are not menu items.
        Repeated presses of a shared tool letter cycle the tools that share it; Shift with the letter
        cycles backwards (paint.net: S ×4 or Shift+S is Magic Wand).

        | What it does | paint.net (Windows) | OpenPainter (Mac) | Where | Note |
        | --- | --- | --- | --- | --- |

        """
        for k in KeyboardShortcuts.canvas {
            md += "| \(cell(k.action)) | \(cell(k.win)) | \(cell(k.mac)) | \(cell(k.scope)) | \(cell(k.note)) |\n"
        }
        md += """

        ## macOS keys left alone

        The test checks every menu key and every key above against this list. The only matches are the
        standard Mac app items (marked "app item"), which are the app's own.

        | Key | macOS uses it for | |
        | --- | --- | --- |

        """
        for s in KeyboardShortcuts.system {
            md += "| \(cell(s.key)) | \(cell(s.what)) | \(s.standardAppItem ? "app item" : "not used by OpenPainter") |\n"
        }
        md += "\n## Mac differences\n\n"
        for n in KeyboardShortcuts.macNotes { md += "- \(n)\n" }
        md += "- paint.net shortcuts that would take a Mac key: see the Note column above (Rotate 90° Clockwise, "
            + "Merge Layer Down, Toggle Layer Visibility, Rotate / Zoom, Redo, Exit).\n"
        md += "\n## Not in this version\n\n| paint.net key | What it does | Why |\n| --- | --- | --- |\n"
        for n in KeyboardShortcuts.notInThisVersion { md += "| \(cell(n.win)) | \(cell(n.action)) | \(cell(n.reason)) |\n" }
        return md
    }

    // MARK: Canvas keys

    func testCanvasKeysCycleToolsChangeTheWidthAndSwapColors() throws {
        let wc = try fx.open()
        let other = try fx.open(name: "second.png")
        let canvas = wc.canvas
        let tools = fx.workspace.tools
        tools.tool = .paintbrush
        key("s", 1, to: canvas)
        XCTAssertEqual(tools.tool, .rectangleSelect)
        key("s", 1, to: canvas)
        XCTAssertEqual(tools.tool, .lassoSelect, "S twice")
        key("S", 1, [.shift], to: canvas)
        XCTAssertEqual(tools.tool, .rectangleSelect, "Shift-S goes back")
        key("S", 1, [.shift], to: canvas)
        XCTAssertEqual(tools.tool, .magicWand, "Shift-S from the first S tool wraps to the last")
        tools.tool = .paintbrush
        key("S", 1, [.shift], to: canvas)
        XCTAssertEqual(tools.tool, .magicWand, "paint.net: Shift+S is Magic Wand")
        key("M", 46, [.shift], to: canvas)
        XCTAssertEqual(tools.tool, .moveSelection, "paint.net: Shift+M is Move Selection")
        key("b", 11, to: canvas)
        XCTAssertEqual(tools.tool, .paintbrush)
        // Brush width: [ and ] by 1, Command by 5 (paint.net Ctrl), within 1...2000.
        tools.settings.brushWidth = 10
        key("]", 30, to: canvas)
        XCTAssertEqual(tools.settings.brushWidth, 11)
        key("[", 33, to: canvas)
        key("[", 33, to: canvas)
        XCTAssertEqual(tools.settings.brushWidth, 9)
        key("]", 30, [.command], to: canvas)
        XCTAssertEqual(tools.settings.brushWidth, 14)
        key("[", 33, [.command], to: canvas)
        XCTAssertEqual(tools.settings.brushWidth, 9)
        tools.settings.brushWidth = 1
        key("[", 33, to: canvas)
        XCTAssertEqual(tools.settings.brushWidth, 1, "never below 1")
        XCTAssertEqual(wc.optionsBar.widthField.stringValue, "1", "the options row follows")
        // X swaps, C moves the notch.
        let p = tools.primary, s = tools.secondary
        key("x", 7, to: canvas)
        XCTAssertEqual(tools.primary, s)
        XCTAssertEqual(tools.secondary, p)
        let slot = tools.activeSlot
        key("c", 8, to: canvas)
        XCTAssertEqual(tools.activeSlot, slot.other)
        // Control-Tab and Control-Shift-Tab move between images.
        fx.workspace.setActive(wc.editor)
        key("\t", 48, [.control], to: canvas)
        XCTAssertTrue(fx.workspace.active === other.editor, "Control-Tab: next image")
        key("\u{19}", 48, [.control, .shift], to: other.canvas)
        XCTAssertTrue(fx.workspace.active === wc.editor, "Control-Shift-Tab: previous image")
    }

    // MARK: Focus

    /// The controls in a view tree a person can operate.
    func controls(in root: NSView) -> [NSView] {
        root.descendantViews.filter { v in
            guard !v.isHiddenOrHasHiddenAncestor, v.frame.width > 0, v.frame.height > 0 else { return false }
            if v is PanelRow || v is SwatchPair || v is PaletteCell || v is ColorWheelView || v is GradientSlider
                || v is ImageStripItem { return true }
            if let t = v as? NSTextField { return t.isEditable && t.isEnabled }
            if let c = v as? NSControl { return c.isEnabled && !(v.superview is NSControl) }
            return false
        }
    }

    /// The views Tab reaches from `start`, following the key view loop.
    func tabLoop(in window: NSWindow) -> [NSView] {
        window.recalculateKeyViewLoop()
        var out: [NSView] = []
        guard let content = window.contentView else { return out }
        var v = content.nextValidKeyView
        while let x = v, !out.contains(where: { $0 === x }), out.count < 2000 {
            out.append(x)
            v = x.nextValidKeyView
        }
        return out
    }

    /// AppKit's own buttons, popups and sliders take Tab focus only while the
    /// system's Keyboard navigation is on; with it off they are skipped and
    /// reached by VoiceOver instead. The test checks the loop for what this
    /// Mac's setting allows and reports the setting.
    func assertEveryControlIsInTheTabLoop(_ window: NSWindow, _ name: String, file: StaticString = #filePath,
                                          line: UInt = #line) -> (reached: Int, total: Int) {
        guard let content = window.contentView else { return (0, 0) }
        let loop = tabLoop(in: window)
        let system = NSApp.isFullKeyboardAccessEnabled
        var missing: [String] = []
        let all = controls(in: content)
        var expected = 0
        for c in all {
            let needsSystem = c is NSControl && !(c is NSTextField)
            if needsSystem && !system { continue }
            expected += 1
            if !loop.contains(where: { $0 === c || ($0 as? NSTextView)?.delegate === c as? NSTextViewDelegate }) {
                missing.append("\(type(of: c)) \(c.accessibilityLabel() ?? (c as? NSButton)?.title ?? "")")
            }
        }
        XCTAssertEqual(missing, [], "\(name): controls Tab never reaches", file: file, line: line)
        return (expected - missing.count, expected)
    }

    func testPanelCycleAndEveryControlInTheTabLoop() throws {
        let wc = try fx.open()
        let window = try XCTUnwrap(wc.window)
        let panels = fx.workspace.panels.panels
        // Next Panel from the image window: Tools, History, Layers, Colors, back.
        let cycler = PanelFocusCycler.shared
        var at: NSWindow? = window
        var order: [String] = []
        for _ in 0..<5 {
            at = cycler.cycle(forward: true, from: at)
            order.append((at as? ShellPanel)?.kind.title ?? (at === window ? "image" : "?"))
        }
        XCTAssertEqual(order, ["Tools", "History", "Layers", "Colors", "image"])
        XCTAssertTrue(window.firstResponder === wc.canvas, "back at the image, the canvas has focus")
        at = cycler.cycle(forward: false, from: window)
        XCTAssertTrue(at === panels[.colors], "Previous Panel from the image is Colors")
        // A focused panel puts focus on its first control (with keyboard access on).
        let layers = panels[.layers]!
        cycler.cycle(forward: true, from: panels[.history])
        XCTAssertTrue(layers.firstResponder is LayerRow || layers.firstResponder is NSButton,
                      "Layers focus: \(String(describing: layers.firstResponder))")
        // A hidden panel leaves the ring.
        fx.workspace.panels.setShown(.history, false)
        XCTAssertTrue(cycler.cycle(forward: true, from: panels[.tools]) === layers)
        fx.workspace.panels.setShown(.history, true)
        // The menu items exist with their keys and are on.
        let m = menu()
        let next = try XCTUnwrap(menuItem(m, key: "`", [.control]))
        XCTAssertEqual(next.action, #selector(PanelFocusCycler.focusNextPanel(_:)))
        XCTAssertNotNil(menuItem(m, key: "`", [.control, .shift]))
        // Rows, swatches and fields are in each panel's Tab loop.
        var report: [String] = []
        for k in PanelKind.allCases {
            let r = assertEveryControlIsInTheTabLoop(panels[k]!, k.title)
            report.append("\(k.title) \(r.reached)/\(r.total)")
        }
        let r = assertEveryControlIsInTheTabLoop(window, "Main window")
        report.append("Main window \(r.reached)/\(r.total)")
        print("TABLOOP keyboard navigation \(NSApp.isFullKeyboardAccessEnabled ? "on" : "off"): \(report.joined(separator: ", "))")
        // Row keys: Down moves to the next row, Return picks it.
        wc.editor.run("layer.add", ["name": "Layer 2"])
        XCTAssertTrue(waitUntil { self.fx.workspace.layersView.layerRows.count == 2 })
        let rows = fx.workspace.layersView.layerRows
        XCTAssertTrue(layers.makeFirstResponder(rows[0]))
        key("", 125, to: rows[0])
        XCTAssertTrue(layers.firstResponder === rows[1], "Down: the row below")
        key("\r", 36, to: rows[1])
        XCTAssertTrue(waitUntil { wc.editor.info.activeLayer == 0 }, "Return on the bottom row picks the bottom layer")
        XCTAssertTrue(layers.firstResponder is LayerRow, "focus stays on a row after the reload")
    }

    // MARK: Keyboard-only end to end

    func testKeyboardOnlyOpenSelectEffectUndoSave() throws {
        // Open: ⌘O is File > Open… (the open panel itself is AppKit's and is
        // driven by keys there); the test opens the chosen file the same way.
        let dir = try ImageIOFixtures.directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("keyboard.png")
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        let savedEngine = ImageDocument.engine, savedWorkspace = ImageDocument.workspace
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        defer {
            ImageDocument.engine = savedEngine
            ImageDocument.workspace = savedWorkspace
        }
        let m = menu()
        let open = try XCTUnwrap(menuItem(m, key: "o", [.command]))
        XCTAssertEqual(open.action, #selector(NSDocumentController.openDocument(_:)))
        let doc = try ImageDocument(contentsOf: url, ofType: "public.png")
        doc.makeWindowControllers()
        for c in doc.windowControllers { c.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        defer {
            for c in doc.windowControllers { c.window?.orderOut(nil) }
            doc.close()
        }
        let wc = try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
        let window = try XCTUnwrap(wc.window)
        let e = wc.editor
        XCTAssertTrue(window.firstResponder === wc.canvas, "the canvas has focus when the image opens")
        let steps = e.info.historyLabels.count
        // Edit: pick the Magic Wand by key (S ×4), then select with keys: ⌘A.
        for _ in 0..<4 { key("s", 1, to: wc.canvas) }
        XCTAssertEqual(fx.workspace.tools.tool, .magicWand)
        pressMenuKey(m, "a", [.command], in: window)
        XCTAssertTrue(waitUntil { e.selection != nil }, "⌘A selects all")
        // Brightness / Contrast: ⇧⌘T opens its dialog (the item's target finds
        // the main window; the test process has none, so it opens on this one).
        let item = try XCTUnwrap(menuItem(m, key: "t", [.command, .shift]))
        let id = try XCTUnwrap(item.representedObject as? String)
        XCTAssertEqual(id, "effect.adjust.brightness_contrast")
        let dialog = try XCTUnwrap(wc.showEffect(id))
        let sheet = try XCTUnwrap(dialog.window)
        XCTAssertTrue(waitUntil { window.attachedSheet === sheet })
        // The first field has focus; type 40, Tab to the next field, type 20.
        let first = try XCTUnwrap(sheet.firstResponder as? NSTextView, "a field editor has focus")
        first.selectAll(nil)
        first.insertText("40", replacementRange: first.selectedRange())
        first.keyDown(with: keyEvent("\t", keyCode: 48, window: sheet))
        let second = try XCTUnwrap(sheet.firstResponder as? NSTextView, "Tab moved to the next field")
        XCTAssertFalse(second === first && second.string == "40", "Tab left the first field")
        second.selectAll(nil)
        second.insertText("20", replacementRange: second.selectedRange())
        second.keyDown(with: keyEvent("\t", keyCode: 48, window: sheet))
        XCTAssertTrue(waitUntil { dialog.isIdle })
        XCTAssertEqual(dialog.values["Brightness"], .int(40))
        XCTAssertEqual(dialog.values["Contrast"], .int(20))
        // Return presses OK (the default button).
        XCTAssertTrue(sheet.defaultButtonCell === dialog.okButton.cell, "OK is the default button")
        // Return: AppKit sends a Return to the key window's default button
        // (NSWindow.defaultButtonCell). The test process is never the active
        // app, so its sheet is never key and NSButton ignores the key; the test
        // makes the same call AppKit makes for Return.
        XCTAssertFalse(dialog.cancelButton.keyEquivalent.isEmpty, "Escape is Cancel")
        sheet.defaultButtonCell?.performClick(nil)
        XCTAssertTrue(waitUntil(30) { e.info.historyLabels.count == steps + 2 }, "Select All and the effect: two steps")
        XCTAssertEqual(e.info.historyLabels.last, "Brightness / Contrast")
        XCTAssertTrue(waitUntil { window.attachedSheet == nil })
        let edited = try e.document.pixels()
        // Undo and Redo by key.
        pressMenuKey(m, "z", [.command], in: window)
        XCTAssertTrue(waitUntil { e.info.historyIndex == steps })
        XCTAssertNotEqual(try e.document.pixels(), edited)
        pressMenuKey(m, "z", [.command, .shift], in: window)
        XCTAssertTrue(waitUntil { e.info.historyIndex == steps + 1 })
        XCTAssertEqual(try e.document.pixels(), edited)
        // Save: ⌘S reaches the document through the responder chain.
        XCTAssertTrue(doc.isDocumentEdited)
        pressMenuKey(m, "s", [.command], in: window)
        XCTAssertTrue(waitUntil(30) { !doc.isDocumentEdited }, "⌘S saved")
        let back = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil).flatMap {
            CGImageSourceCreateImageAtIndex($0, 0, nil)
        })
        XCTAssertEqual(back.width, 512)
        let reopened = try fx.engine.openPNG(path: url.path)
        defer { try? reopened.close() }
        XCTAssertEqual(try reopened.pixels(), edited, "the file holds the edit")
    }

    // MARK: VoiceOver

    /// Every control in the tree has a name VoiceOver can read.
    func unnamedControls(_ nodes: [AXNode]) -> [String] {
        // AppKit names the window buttons and scroller parts itself (VoiceOver
        // reads their subrole: "close button", "increment arrow").
        let systemParts: Set<String> = ["AXCloseButton", "AXZoomButton", "AXMinimizeButton", "AXFullScreenButton",
                                        "AXIncrementArrow", "AXDecrementArrow", "AXIncrementPage", "AXDecrementPage"]
        return nodes.filter { n in
            controlRoles.contains(NSAccessibility.Role(rawValue: n.role)) && n.name.isEmpty
                && n.role != NSAccessibility.Role.list.rawValue && !systemParts.contains(n.subrole)
        }.map { n in
            var s = n.line.trimmingCharacters(in: .whitespaces)
            if let v = n.element as? NSView { s += " <\(type(of: v)) in \(type(of: v.superview ?? v))>" }
            if let c = n.element as? NSCell, let v = c.controlView { s += " <\(type(of: c)) of \(type(of: v))>" }
            return s
        }
    }

    func testVoiceOverWalkthroughAndEveryControlIsNamed() throws {
        let wc = try fx.open()
        let window = try XCTUnwrap(wc.window)
        let e = wc.editor
        let ws = fx.workspace
        XCTAssertTrue(waitUntil { e.thumbnail != nil })
        var md = """
        # VoiceOver walkthrough

        Generated by `KeyboardAccessibilityTests.testVoiceOverWalkthroughAndEveryControlIsNamed`. The test
        reads the NSAccessibility tree of the running window, panels and dialogs (what VoiceOver reads:
        role, name, value, help) and performs actions through it (`accessibilityPerformPress`), then
        writes this file. Do not edit by hand. Each control role (button, field, popup, slider, row, image)
        must have a name; the last section lists any that do not.

        Each line: role "name" = value [off] — help. Indents show nesting.


        """
        var unnamed: [String] = []
        func section(_ title: String, _ root: NSAccessibilityProtocol, maxDepth: Int = 30) {
            let nodes = axTree(root, maxDepth: maxDepth)
            unnamed += unnamedControls(nodes).map { "\(title): \($0)" }
            md += "## \(title)\n\n```\n" + nodes.map(\.line).joined(separator: "\n") + "\n```\n\n"
        }
        func step(_ text: String) { md += "- \(text)\n" }

        section("Main window", window)
        // The canvas announces the image size, zoom and tool.
        let canvasValue = axString(wc.canvas.accessibilityValue())
        XCTAssertTrue(canvasValue.contains("512 by 384 pixels"), canvasValue)
        XCTAssertTrue(canvasValue.contains("zoom"), canvasValue)
        XCTAssertTrue(canvasValue.contains("Paintbrush"), canvasValue)
        XCTAssertEqual(wc.canvas.accessibilityRole(), .image)

        md += "## Actions\n\n"
        // Tools: press Magic Wand through accessibility.
        let toolsTree = axTree(ws.panels.panels[.tools]!)
        let wand = try XCTUnwrap(toolsTree.first { $0.name == "Magic Wand" }, "Magic Wand button")
        _ = wand.element.accessibilityPerformPress()
        XCTAssertEqual(ws.tools.tool, .magicWand)
        step("Tools panel: press \"Magic Wand\". The tool is now \(ws.tools.tool.label). "
             + "Canvas value: \(axString(wc.canvas.accessibilityValue()))")
        XCTAssertTrue(axString(wc.canvas.accessibilityValue()).contains("Magic Wand"))
        // Layers: Add New Layer, then press the bottom row.
        let layersTree = axTree(ws.panels.panels[.layers]!)
        let add = try XCTUnwrap(layersTree.first { $0.name.hasPrefix("Add New Layer") }, "Add New Layer button")
        _ = add.element.accessibilityPerformPress()
        XCTAssertTrue(waitUntil { e.info.layers.count == 2 })
        step("Layers panel: press \"\(add.name)\". Layers: \(e.info.layers.map(\.name).joined(separator: ", ")); active: \(e.info.layers[e.info.activeLayer].name).")
        let rows = axTree(ws.panels.panels[.layers]!).filter { $0.element is LayerRow }
        XCTAssertEqual(rows.count, 2)
        // Press the row that is not the active layer; it becomes active.
        let target = try XCTUnwrap(rows.first { ($0.element as? LayerRow)?.isCurrent == false })
        let targetIndex = try XCTUnwrap((target.element as? LayerRow)?.index)
        _ = target.element.accessibilityPerformPress()
        XCTAssertTrue(waitUntil { e.info.activeLayer == targetIndex })
        step("Layers panel: press the row \"\(target.name)\" (value \(target.value)). Active layer: \(e.info.layers[e.info.activeLayer].name).")
        // History: press the first row (jump back).
        let historyRows = axTree(ws.panels.panels[.history]!).filter { $0.element is HistoryRow }
        XCTAssertGreaterThanOrEqual(historyRows.count, 2)
        XCTAssertTrue(historyRows[0].element.accessibilityPerformPress())
        XCTAssertTrue(waitUntil { e.info.historyIndex == 0 })
        let rowValues = axTree(ws.panels.panels[.history]!).filter { $0.element is HistoryRow }
            .map { "\($0.name) = \($0.value)" }
        step("History panel: press \"\(historyRows[0].name)\". Rows now: \(rowValues.joined(separator: "; ")).")
        // Colors: the swap pair and a palette swatch.
        let colorsTree = axTree(ws.panels.panels[.colors]!)
        let pair = try XCTUnwrap(colorsTree.first { $0.element is SwatchPair })
        let slot = ws.tools.activeSlot
        XCTAssertTrue(pair.element.accessibilityPerformPress())
        XCTAssertEqual(ws.tools.activeSlot, slot.other)
        step("Colors panel: press \"\(pair.name)\". The active slot is now \(ws.tools.activeSlot).")
        let swatch = try XCTUnwrap(colorsTree.first { $0.element is PaletteCell })
        XCTAssertTrue(swatch.element.accessibilityPerformPress())
        step("Colors panel: press \"\(swatch.name)\". Primary \(ws.tools.primary.hex), secondary \(ws.tools.secondary.hex).")
        // The status bar reads its values.
        let status = axTree(wc.statusBar).filter { !$0.value.isEmpty || $0.role == NSAccessibility.Role.button.rawValue }
        step("Status bar: " + status.map { [$0.name, $0.value].filter { !$0.isEmpty }.joined(separator: " = ") }
            .filter { !$0.isEmpty }.joined(separator: "; "))
        md += "\n"

        for k in PanelKind.allCases {
            let panel = ws.panels.panels[k]!
            section("\(k.title) panel", panel)
            XCTAssertTrue(axTree(panel).contains { $0.role == "AXHeading" && $0.name == k.title }, "\(k.title) heading")
        }

        // Dialogs.
        try forEachDialog(wc) { name, sheet in section("\(name) dialog", sheet) }
        md += "## Unnamed controls\n\n" + (unnamed.isEmpty ? "None.\n" : unnamed.map { "- \($0)" }.joined(separator: "\n") + "\n")
        try md.write(to: repoRoot.appendingPathComponent("docs/accessibility-walkthrough.md"), atomically: true,
                     encoding: .utf8)
        XCTAssertEqual(unnamed, [], "controls with no VoiceOver name")
    }

    /// Opens each dialog on `wc`, calls `body` with its window, and closes it.
    func forEachDialog(_ wc: EditorWindowController, _ body: (String, NSWindow) throws -> Void) throws {
        let window = try XCTUnwrap(wc.window)
        let e = wc.editor
        func closeSheet() {
            if let s = window.attachedSheet { window.endSheet(s); s.orderOut(nil) }
            XCTAssertTrue(waitUntil { window.attachedSheet == nil })
        }
        for id in ["effect.adjust.brightness_contrast", "effect.adjust.curves", "effect.adjust.levels",
                   "effect.adjust.hue_saturation", "effect.blur.motion", "effect.distort.twist", "effect.render.clouds",
                   "effect.noise.add", "effect.stylize.emboss"] {
            guard let d = wc.showEffect(id), let sheet = d.window else { continue }
            XCTAssertTrue(waitUntil { d.isIdle })
            try body(d.schema.label, sheet)
            d.cancel(nil)
            XCTAssertTrue(waitUntil { window.attachedSheet == nil })
        }
        // The sheet hook translates a sheet once it is attached; spin once so
        // it has run (as it has by the time a person sees the sheet).
        func settle() { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
        if let d = wc.presentResizeImage(), let s = d.window { settle(); try body("Resize Image", s); d.cancel(nil); closeSheet() }
        if let d = wc.presentCanvasSize(), let s = d.window { settle(); try body("Canvas Size", s); d.cancel(nil); closeSheet() }
        if let d = wc.showLayerProperties(index: e.info.activeLayer), let s = d.window {
            settle()
            try body("Layer Properties", s)
            d.cancel(nil)
            closeSheet()
        }
        if let d = wc.showColorProfile(), let s = d.window { settle(); try body("Color Profile", s); d.cancel(nil); closeSheet() }
        e.run("selection.select_all")
        XCTAssertTrue(waitUntil { e.selection != nil })
        if let d = wc.presentExpandSelection() {
            XCTAssertTrue(waitUntil { window.attachedSheet != nil })
            settle()
            if let s = window.attachedSheet { try body("Expand Selection", s) }
            closeSheet()
            _ = d
        }
        let newImage = NewImageController()
        if let s = newImage.window { L10n.localizeDialog(s); try body("New Image", s); s.orderOut(nil) }
        let px = SavePixels(rgba: [UInt8](repeating: 128, count: 64 * 64 * 4), width: 64, height: 64, icc: Data(), dpi: 96)
        for type in ["public.jpeg", "public.png"] {
            let save = SaveOptionsController(type: type, options: SaveOptions(), pixels: px, metadata: nil)
            if let s = save.window {
                L10n.localizeDialog(s)
                try body("Save Options " + (type == "public.png" ? "PNG" : "JPEG"), s)
                s.orderOut(nil)
            }
        }
        let settings = SettingsWindowController(settings: fx.workspace.settings)
        if let s = settings.window { L10n.localizeDialog(s); try body("Settings", s); s.close() }
    }

    // MARK: Strings

    /// Every string the windows, panels, menus and dialogs showed is in the
    /// String Catalog. With TEST_RUNNER_M59_WRITE_STRINGS=1 the test writes the
    /// catalog from what it saw (keeping existing entries and translations).
    func testEveryShownStringIsInTheStringCatalog() throws {
        L10n.resetUsed()
        L10n.recording = true
        let wc = try fx.open()
        _ = menu()
        for t in PaintTool.allCases {
            _ = L10n.tr(t.label, "Tool name (Tools panel, tool popup, tooltips)")
            _ = L10n.tr(t.hint, "Status bar hint for the current tool")
        }
        for e in fx.workspace.effects { _ = L10n.engine(e.id, english: e.label) }
        for k in PanelKind.allCases { _ = L10n.tr(k.title, "Floating panel title") }
        try forEachDialog(wc) { _, _ in }
        L10n.recording = false
        let used = L10n.used
        XCTAssertGreaterThan(used.count, 300, "strings seen")
        let url = repoRoot.appendingPathComponent("mac/OpenPainterKit/Sources/Shell/Localizable.xcstrings")
        var json = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any] ?? [:]
        var strings = json["strings"] as? [String: Any] ?? [:]
        if ProcessInfo.processInfo.environment["M59_WRITE_STRINGS"] == "1" {
            for (k, comment) in used where strings[k] == nil {
                var entry: [String: Any] = ["comment": comment, "extractionState": "manual"]
                if k.hasPrefix("engine."), let english = comment.components(separatedBy: "English: ").last {
                    entry["localizations"] = ["en": ["stringUnit": ["state": "translated", "value": english]]]
                }
                strings[k] = entry
            }
            json["strings"] = strings
            json["sourceLanguage"] = "en"
            json["version"] = "1.0"
            let data = try JSONSerialization.data(withJSONObject: json, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: url)
        }
        let missing = used.keys.filter { strings[$0] == nil }.sorted()
        XCTAssertEqual(missing, [], "strings shown but not in \(url.lastPathComponent) (run with M59_WRITE_STRINGS=1)")
        // Each entry has a comment for translators.
        for (k, v) in strings {
            XCTAssertFalse(((v as? [String: Any])?["comment"] as? String ?? "").isEmpty, "\(k) has no comment")
        }
        // The compiled catalog is in the framework bundle.
        let bundle = Bundle(for: Workspace.self)
        XCTAssertNotNil(bundle.path(forResource: "Localizable", ofType: "strings", inDirectory: nil,
                                    forLocalization: "en"), "Localizable.strings in \(bundle.bundlePath)")
        // Engine labels are keyed by command id, with the English table in the
        // catalog. In English the engine's own label shows.
        XCTAssertEqual(bundle.localizedString(forKey: "engine.effect.blur.gaussian", value: "x", table: nil), "Gaussian Blur")
        XCTAssertEqual(L10n.engine("effect.blur.gaussian", english: "Gaussian Blur"), "Gaussian Blur")
        for e in fx.workspace.effects {
            XCTAssertNotNil(strings["engine." + e.id], "engine.\(e.id) in the English table")
        }
    }

    /// Text whose drawn width is wider than its view: clipped or truncated.
    func clipped(in root: NSView, except: [NSView] = []) -> [String] {
        var out: [String] = []
        for v in [root] + root.descendantViews where !v.isHiddenOrHasHiddenAncestor && v.frame.width > 0
            && !except.contains(where: { $0 === v }) {
            var text = "", need: CGFloat = 0, have = v.bounds.width
            switch v {
            case let p as NSPopUpButton:
                text = p.titleOfSelectedItem ?? ""
                need = p.cell?.cellSize.width ?? 0
            case let s as NSSegmentedControl:
                for i in 0..<s.segmentCount where s.width(forSegment: i) > 0 {
                    let l = s.label(forSegment: i) ?? ""
                    let w = (l as NSString).size(withAttributes: [.font: s.font ?? NSFont.systemFont(ofSize: 12)]).width + 12
                    if w > s.width(forSegment: i) + 0.5 { out.append("segment \"\(l)\" needs \(Int(w)) has \(Int(s.width(forSegment: i)))") }
                }
                continue
            case let b as NSButton where b.imagePosition != .imageOnly:
                text = b.title
                need = b.cell?.cellSize.width ?? 0
            case let f as NSTextField where !f.isEditable:
                text = f.stringValue
                if f.cell?.wraps == true || f.maximumNumberOfLines == 0 && f.lineBreakMode == .byWordWrapping {
                    let size = f.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: f.bounds.width, height: 10_000)) ?? .zero
                    if text.hasPrefix("["), size.height > f.bounds.height + 1 { out.append("\"\(text)\" wraps to \(Int(size.height)) pt in \(Int(f.bounds.height))") }
                    continue
                }
                need = f.cell?.cellSize.width ?? 0
            default:
                continue
            }
            // Only translated text counts (file names and numbers are data).
            guard text.hasPrefix("["), need > have + 1 else { continue }
            out.append("\(type(of: v)) \"\(text)\" needs \(Int(need)) pt, has \(Int(have))")
        }
        return out
    }

    func render(_ view: NSView, _ name: String) throws {
        guard let dir = pngDir else { return }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        for (look, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            view.window?.appearance = NSAppearance(named: appearance)
            view.appearance = NSAppearance(named: appearance)
            view.needsLayout = true
            view.layoutSubtreeIfNeeded()
            for v in [view] + view.descendantViews { v.needsDisplay = true }
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            let url = dir.appendingPathComponent("m59-\(name)-\(look).png")
            try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: url)
        }
        view.window?.appearance = nil
        view.appearance = nil
    }

    func testPseudoLocalizationRendersWithoutClipping() throws {
        L10n.pseudo = true
        XCTAssertEqual(L10n.pseudoString("Open").count, "Open".count * 2 + 3)
        let wc = try fx.open()
        let window = try XCTUnwrap(wc.window)
        XCTAssertTrue(waitUntil { wc.editor.thumbnail != nil })
        XCTAssertTrue(window.title.hasPrefix("[") == false, "the image name is not translated")
        XCTAssertTrue(wc.statusBar.subviews.contains { ($0 as? NSTextField)?.stringValue.hasPrefix("[") == true }
                      || wc.statusBar.descendantViews.contains { ($0 as? NSTextField)?.stringValue.hasPrefix("[") == true },
                      "status bar labels are pseudo")
        // A rebuilt options row (another tool) is translated too.
        fx.workspace.tools.tool = .magicWand
        XCTAssertTrue(wc.optionsBar.descendantViews.contains { ($0 as? NSTextField)?.stringValue.hasPrefix("[Ţöļéŕåñçé") == true },
                      "the Magic Wand row is pseudo")
        fx.workspace.tools.tool = .paintbrush
        var report: [String: [String]] = [:]
        window.contentView?.layoutSubtreeIfNeeded()
        // The status hint gives way first by design (SPEC: "On a tight window
        // the hint hides first"), so it may truncate.
        report["Main window"] = clipped(in: window.contentView!, except: [wc.statusBar.hint])
        try render(window.contentView!, "main")
        for k in PanelKind.allCases {
            let p = fx.workspace.panels.panels[k]!
            p.contentView?.layoutSubtreeIfNeeded()
            report["\(k.title) panel"] = clipped(in: p.contentView!)
            try render(p.contentView!, "panel-\(k.rawValue)")
        }
        try forEachDialog(wc) { name, sheet in
            L10n.localizeDialog(sheet)
            sheet.contentView?.layoutSubtreeIfNeeded()
            report["\(name) dialog"] = clipped(in: sheet.contentView!)
            try render(sheet.contentView!, "dialog-" + name.lowercased().replacingOccurrences(of: " ", with: "-")
                .replacingOccurrences(of: "/", with: "").replacingOccurrences(of: ":", with: ""))
        }
        let lines = report.sorted { $0.key < $1.key }.flatMap { k, v in v.map { "\(k): \($0)" } }
        for l in lines { print("CLIPPED \(l)") }
        print("CLIPPED total \(lines.count)")
        XCTAssertEqual(lines, [], "pseudo-localized text that does not fit")
    }
}
