// Menus complete (M55): the live menu bar against the paint.net 5.1 catalog
// (tests/catalog/paintnet-5.1-menus.toml), the same check as
// tools/catalog/compare-menus.py, run in process; unique shortcuts; icons;
// enabled states; the M49 Layers items; Save All; Repeat with seeds.
import AppKit
@testable import OpenPainterKit
import XCTest

/// The catalog, read with a small parser for the subset of TOML it uses:
/// comments, `key = []`, `[[item]]` and `key = "string"` or `key = true`.
struct MenuCatalog {
    var items: [[String: String]] = []
    var shared: Set<String> = []

    static let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .appendingPathComponent("../../../tests/catalog/paintnet-5.1-menus.toml").standardized

    static func load() throws -> MenuCatalog {
        var c = MenuCatalog()
        var current: [String: String]?
        for raw in try String(contentsOf: url, encoding: .utf8).split(separator: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            if line == "[[item]]" {
                if let current { c.items.append(current) }
                current = [:]
                continue
            }
            guard let eq = line.range(of: " = ") else { throw NSError(domain: "catalog: \(line)", code: 1) }
            let key = String(line[..<eq.lowerBound]), value = String(line[eq.upperBound...])
            let parsed = try JSONSerialization.jsonObject(with: Data(value.utf8), options: .fragmentsAllowed)
            if current == nil {
                if key == "shared_shortcuts" { c.shared = Set(parsed as? [String] ?? []) }
                continue
            }
            if let b = parsed as? Bool, value == "true" || value == "false" {
                current?[key] = b ? "true" : "false"
            } else if let s = parsed as? String {
                current?[key] = s
            } else {
                throw NSError(domain: "catalog value: \(line)", code: 2)
            }
        }
        if let current { c.items.append(current) }
        return c
    }
}

/// One visible menu item.
struct LiveItem {
    let path: String
    let key: String
    let off: Bool
    let icon: Bool
    let command: String
    let isSubmenu: Bool
    let item: NSMenuItem
}

@MainActor
func liveItems(_ main: NSMenu) -> [LiveItem] {
    var out: [LiveItem] = []
    func walk(_ menu: NSMenu, _ path: String) {
        for item in menu.items where !item.isHidden && !item.isSeparatorItem {
            let here = path.isEmpty ? item.title : "\(path) > \(item.title)"
            var command = ""
            if let id = item.representedObject as? String {
                command = id
            } else if let a = item.action {
                command = NSStringFromSelector(a)
            }
            out.append(LiveItem(path: here, key: item.keyEquivalent.isEmpty ? "" : MenuSkeleton.shortcut(item),
                                off: item.submenu == nil && item.action == nil, icon: item.image != nil,
                                command: command, isSubmenu: item.submenu != nil, item: item))
            if let sub = item.submenu { walk(sub, here) }
        }
    }
    walk(main, "")
    return out
}

/// The comparison of tools/catalog/compare-menus.py. Returns (differences, allowed).
@MainActor
func compareMenus(_ catalog: MenuCatalog, _ live: [LiveItem]) -> ([String], [String]) {
    var failures: [String] = [], allowed: [String] = []
    func parent(_ p: String) -> String {
        guard let r = p.range(of: " > ", options: .backwards) else { return "" }
        return String(p[..<r.lowerBound])
    }
    let expected = catalog.items.filter { !($0["mac"] ?? "").isEmpty }
    var byPath: [String: [String: String]] = [:]
    for i in expected {
        if byPath[i["mac"]!] != nil { failures.append("CATALOG  \(i["mac"]!) is listed twice") }
        byPath[i["mac"]!] = i
    }
    for i in catalog.items {
        if i["status"] == "not in v1" {
            if !(i["mac"] ?? "").isEmpty { failures.append("CATALOG  \(i["pdn"]!) is not in v1 but has a Mac path") }
            allowed.append("NOT IN V1  \(i["pdn"]!): \(i["reason"] ?? "")")
        } else if i["status"] == "pending", (i["mac"] ?? "").isEmpty {
            allowed.append("PENDING    \(i["pdn"]!) (absent): \(i["reason"] ?? "")")
        }
    }
    let liveByPath = Dictionary(live.map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
    for p in expected.map({ $0["mac"]! }) where liveByPath[p] == nil { failures.append("MISSING  \(p)") }
    for l in live where byPath[l.path] == nil { failures.append("EXTRA    \(l.path)") }
    var parents: [String] = []
    for i in expected where !parents.contains(parent(i["mac"]!)) { parents.append(parent(i["mac"]!)) }
    for par in parents {
        let want = expected.map { $0["mac"]! }.filter { parent($0) == par && liveByPath[$0] != nil }
        let got = live.map(\.path).filter { parent($0) == par && byPath[$0] != nil }
        if want != got { failures.append("ORDER    \(par.isEmpty ? "(menu bar)" : par): expected \(want) got \(got)") }
    }
    for l in live {
        guard let i = byPath[l.path] else { continue }
        if l.key != i["key"] { failures.append("SHORTCUT \(l.path): expected [\(i["key"]!)] got [\(l.key)]") }
        if i["status"] == "pending" {
            if !l.off { failures.append("STATE    \(l.path): pending items stay off") }
            allowed.append("PENDING    \(l.path) (off): \(i["reason"] ?? "")")
        } else if l.off {
            failures.append("STATE    \(l.path): off, but the catalog says it is built (\(i["status"]!))")
        }
        let icon = i["icon"] == "true"
        if icon, !l.icon { failures.append("ICON     \(l.path): paint.net shows an icon; the Mac item has none") }
        // macOS adds its own icons to standard Mac items (Hide, Minimize, Revert).
        if l.icon, !icon, !(i["pdn"] ?? "").isEmpty {
            failures.append("ICON     \(l.path): has an icon paint.net does not show")
        }
        if !l.isSubmenu, i["command"] != l.command {
            failures.append("COMMAND  \(l.path): expected \(i["command"]!) got \(l.command)")
        }
    }
    var seen: [String: [String]] = [:]
    for l in live where !l.key.isEmpty && !catalog.shared.contains(l.key) { seen[l.key, default: []].append(l.path) }
    for (k, paths) in seen.sorted(by: { $0.key < $1.key }) where paths.count > 1 {
        failures.append("DUPLICATE [\(k)]: \(paths)")
    }
    return (failures, allowed)
}

@MainActor
final class MenuCatalogTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
        EffectDialogController.lastEffect = nil
    }

    override func tearDown() async throws {
        EffectDialogController.lastEffect = nil
        fx.tearDown()
    }

    func menu() -> NSMenu { MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: fx.workspace.menuTarget) }

    func item(_ menu: NSMenu, _ path: String) throws -> NSMenuItem {
        let parts = path.components(separatedBy: " > ")
        var m = menu
        for p in parts.dropLast() { m = try XCTUnwrap(m.item(withTitle: p)?.submenu, p) }
        return try XCTUnwrap(m.item(withTitle: parts.last!), path)
    }

    // MARK: Catalog

    /// The comparison passes: the only differences are "not in v1" and
    /// "pending" items. Also writes mac/build/menu-live.tsv for the script.
    func testLiveMenuBarMatchesTheCatalog() throws {
        let catalog = try MenuCatalog.load()
        XCTAssertGreaterThan(catalog.items.count, 150)
        let live = liveItems(menu())
        let (failures, allowed) = compareMenus(catalog, live)
        for line in allowed + failures { print(line) }
        XCTAssertEqual(failures, [], "differences from \(MenuCatalog.url.path)")
        // Only not-in-v1 and pending items may differ.
        XCTAssertTrue(allowed.allSatisfy { $0.hasPrefix("NOT IN V1") || $0.hasPrefix("PENDING") })
        let tsv = live.map { "\($0.path)\t\($0.key)\t\($0.off ? "off" : "on")\t\($0.icon ? "icon" : "-")\t\($0.command)" }
        let out = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appendingPathComponent("../../build/menu-live.tsv").standardized
        try FileManager.default.createDirectory(at: out.deletingLastPathComponent(), withIntermediateDirectories: true)
        try (["# The live menu bar (MenuCatalogTests), for tools/catalog/compare-menus.py."] + tsv)
            .joined(separator: "\n").write(to: out, atomically: true, encoding: .utf8)
    }

    /// The check finds each kind of difference (so a pass means something).
    func testTheComparisonReportsEachKindOfDifference() throws {
        let catalog = try MenuCatalog.load()
        let m = menu()
        let edit = try item(m, "Edit").submenu!
        try item(m, "Edit > Deselect").keyEquivalent = "e"  // wrong shortcut; ⌘E is free
        edit.removeItem(try item(m, "Edit > Select All"))  // missing
        edit.addItem(withTitle: "Extra Thing", action: #selector(NSResponder.selectAll(_:)), keyEquivalent: "")  // extra
        let image = try item(m, "Image").submenu!
        let flatten = try item(m, "Image > Flatten")
        image.removeItem(flatten)
        image.insertItem(flatten, at: 0)  // wrong order
        try item(m, "Layers > Delete Layer").image = nil  // missing icon
        try item(m, "Layers > Flip Vertical").action = nil  // built item off
        try item(m, "View > Rulers").keyEquivalent = "'"  // shares Pixel Grid's ⌘'
        try item(m, "View > Rulers").keyEquivalentModifierMask = [.command]
        let (failures, _) = compareMenus(catalog, liveItems(m))
        for prefix in ["SHORTCUT Edit > Deselect", "MISSING  Edit > Select All", "EXTRA    Edit > Extra Thing",
                       "ORDER    Image", "ICON     Layers > Delete Layer", "STATE    Layers > Flip Vertical",
                       "DUPLICATE [⌘']"] {
            XCTAssertTrue(failures.contains { $0.hasPrefix(prefix) }, "\(prefix) in \(failures)")
        }
    }

    func testEveryShortcutIsUniqueAndEveryIconExists() throws {
        let catalog = try MenuCatalog.load()
        XCTAssertEqual(catalog.shared, [], "paint.net 5.1's own menus share no shortcut")
        let live = liveItems(menu())
        var seen: [String: String] = [:]
        for l in live where !l.key.isEmpty {
            XCTAssertNil(seen[l.key], "\(l.path) and \(seen[l.key] ?? "") share \(l.key)")
            seen[l.key] = l.path
        }
        // The hidden panel twins (Option-Cmd-1 to 4) are unique too.
        let hidden = menu().item(withTitle: "Window")!.submenu!.items.filter(\.isHidden)
        XCTAssertEqual(hidden.count, 4)
        for h in hidden { XCTAssertNil(seen["⌥⌘\(h.keyEquivalent)"], h.title) }
        // Every icon is a real SF Symbol, and every catalog icon item has one.
        for (path, symbol) in MenuIcons.symbols {
            XCTAssertNotNil(NSImage(systemSymbolName: symbol, accessibilityDescription: nil), "\(path): \(symbol)")
        }
        let iconPaths = Set(catalog.items.filter { $0["icon"] == "true" && !($0["mac"] ?? "").isEmpty }.map { $0["mac"]! })
        for p in iconPaths { XCTAssertNotNil(MenuIcons.symbol(for: p), p) }
        let macOnly = Set(catalog.items.filter { ($0["pdn"] ?? "").isEmpty }.compactMap { $0["mac"] })
        for l in live where l.icon { XCTAssertTrue(iconPaths.contains(l.path) || macOnly.contains(l.path), l.path) }
    }

    // MARK: Enabled states

    /// With no document, every document command is off; app, window-list and
    /// document-controller items stay on.
    func testWithNoDocumentDocumentItemsAreOff() throws {
        for w in NSApp.windows where w is EditorWindow { w.orderOut(nil) }
        let m = menu()
        let appLevel: Set<String> = [
            "OpenPainter > About OpenPainter", "OpenPainter > Settings…", "OpenPainter > Hide OpenPainter",
            "OpenPainter > Hide Others", "OpenPainter > Show All", "OpenPainter > Quit OpenPainter",
            "File > New…", "File > Open…", "File > Open Recent > Clear Menu", "Window > Bring All to Front",
            "Window > Tools", "Window > History", "Window > Layers", "Window > Colors", "Window > Reset Panel Layout",
        ]
        for l in liveItems(m) where !l.isSubmenu && !l.off && !appLevel.contains(l.path) {
            let item = l.item
            if let t = item.target as? NSMenuItemValidation {
                XCTAssertFalse(t.validateMenuItem(item), "\(l.path) is off with no document")
                continue
            }
            guard let action = item.action else { continue }
            // Untargeted items go to the first responder that implements the
            // action; with no document window, none of ours is reachable.
            let target = NSApp.target(forAction: action, to: nil, from: item)
            XCTAssertFalse(target is EditorWindowController || target is EditorWindow || target is MetalCanvasView,
                           "\(l.path) reaches \(String(describing: target))")
            if let v = target as? NSMenuItemValidation, !(target is NSDocumentController) {
                XCTAssertFalse(v.validateMenuItem(item), "\(l.path) is off with no document")
            }
        }
        // Save All with nothing edited is off.
        XCTAssertFalse(ImageDocumentController.canSaveAll([]))
    }

    /// One layer, no selection: the items that need a second layer, a
    /// selection or a layer above or below are off; then they turn on.
    func testItemsAreOffExactlyWhenTheyCannotAct() throws {
        let wc = try fx.open()
        let e = wc.editor
        let m = menu()
        func on(_ path: String) throws -> Bool { wc.validateMenuItem(try item(m, path)) }
        let alwaysOn = ["Image > Flip Horizontal", "Image > Rotate 180°", "Image > Resize…", "Layers > Add New Layer",
                        "Layers > Duplicate Layer", "Layers > Flip Horizontal", "Layers > Flip Vertical",
                        "Layers > Rotate 180°", "Layers > Import From File…", "Layers > Layer Properties…",
                        "Edit > Select All", "Adjustments > Invert Colors", "Effects > Blurs > Gaussian Blur"]
        let oneLayerOff = ["Image > Flatten", "Layers > Delete Layer", "Layers > Merge Layer Down",
                           "Layers > Move Layer to Top", "Layers > Move Layer Up", "Layers > Move Layer Down",
                           "Layers > Move Layer to Bottom", "Layers > Go to Top Layer", "Layers > Go to Layer Above",
                           "Layers > Go to Layer Below", "Layers > Go to Bottom Layer"]
        let selectionOff = ["Edit > Deselect", "Edit > Invert Selection", "Edit > Erase Selection",
                            "Edit > Fill Selection", "Image > Crop to Selection", "View > Zoom to Selection",
                            "Edit > Content-Aware Fill"]
        for p in alwaysOn { XCTAssertTrue(try on(p), "\(p) on") }
        for p in oneLayerOff + selectionOff { XCTAssertFalse(try on(p), "\(p) off with one layer and no selection") }
        let window = try XCTUnwrap(wc.window as? EditorWindow)
        XCTAssertFalse(window.validateUserInterfaceItem(try item(m, "Edit > Undo")), "nothing to undo")
        XCTAssertFalse(try on("Effects > Repeat Effect"), "nothing to repeat")
        // Paste Selection with nothing on the clipboard is off.
        let saved = SelectionClipboard.pasteboard
        SelectionClipboard.pasteboard = NSPasteboard(name: .init("OpenPainterTests.\(UUID().uuidString)"))
        defer { SelectionClipboard.pasteboard = saved }
        SelectionClipboard.pasteboard.clearContents()
        XCTAssertFalse(try on("Edit > Paste Selection > Replace"), "nothing to paste")

        // A selection turns the selection items on.
        wc.selectAllPixels()
        XCTAssertTrue(waitUntil { e.selection != nil })
        for p in selectionOff { XCTAssertTrue(try on(p), "\(p) on with a selection") }
        XCTAssertTrue(try on("Edit > Copy Selection"))
        wc.runCommand(try item(m, "Edit > Deselect"))
        XCTAssertTrue(waitUntil { e.selection == nil })

        // A second layer (added on top, active): items for a layer below turn on.
        wc.addLayer()
        XCTAssertTrue(waitUntil { e.info.layers.count == 2 })
        wc.goToLayer(try item(m, "Layers > Go to Top Layer"))
        XCTAssertTrue(waitUntil { e.info.activeLayer == 1 })
        for p in ["Image > Flatten", "Layers > Delete Layer", "Layers > Merge Layer Down", "Layers > Move Layer Down",
                  "Layers > Move Layer to Bottom", "Layers > Go to Layer Below", "Layers > Go to Bottom Layer"] {
            XCTAssertTrue(try on(p), "\(p) on with a layer below")
        }
        for p in ["Layers > Move Layer Up", "Layers > Move Layer to Top", "Layers > Go to Layer Above",
                  "Layers > Go to Top Layer"] {
            XCTAssertFalse(try on(p), "\(p) off on the top layer")
        }
        XCTAssertTrue(window.validateUserInterfaceItem(try item(m, "Edit > Undo")))
        // A dialog up: document commands are off.
        let dialog = try XCTUnwrap(wc.showEffect("effect.blur.gaussian"))
        for p in ["Image > Flatten", "Layers > Flip Horizontal", "Layers > Move Layer to Bottom",
                  "Layers > Import From File…", "Adjustments > Invert Colors"] {
            XCTAssertFalse(try on(p), "\(p) off while a dialog is up")
        }
        dialog.cancel(nil)
        XCTAssertTrue(waitUntil { wc.window?.attachedSheet == nil })
    }

    // MARK: Layers items (M49 engine commands)

    func testLayerMenuItemsRun() throws {
        let wc = try fx.open()
        let e = wc.editor
        let m = menu()
        let w = e.info.width
        let before = try e.document.pixels()
        func pixel(_ px: [UInt8], _ x: Int, _ y: Int) -> ArraySlice<UInt8> { px[(y * w + x) * 4..<(y * w + x) * 4 + 4] }
        wc.runCommand(try item(m, "Layers > Flip Horizontal"))
        XCTAssertTrue(waitUntil { e.info.historyLabels.last == "Flip Layer Horizontal" })
        var px = try e.document.pixels()
        XCTAssertEqual(pixel(px, 0, 0), pixel(before, w - 1, 0))
        wc.runCommand(try item(m, "Layers > Flip Vertical"))
        XCTAssertTrue(waitUntil { e.info.historyLabels.last == "Flip Layer Vertical" })
        wc.runCommand(try item(m, "Layers > Rotate 180°"))
        XCTAssertTrue(waitUntil { e.info.historyLabels.last == "Rotate Layer 180°" })
        px = try e.document.pixels()
        XCTAssertEqual(px, before, "flip H, flip V and rotate 180° give the original back")
        XCTAssertEqual(e.info.historyLabels.count, 4)

        // Import From File adds a layer above the active one.
        wc.importLayer(from: fixtures.appendingPathComponent("plasma_alpha_512x384.png"))
        XCTAssertTrue(waitUntil { e.info.layers.count == 2 }, e.message ?? "")
        XCTAssertEqual(e.info.historyLabels.last, "Import From File")
        XCTAssertEqual(e.info.activeLayer, 1)

        // Move Layer to Bottom, then to Top.
        let top = e.info.layers[1].name
        XCTAssertTrue(wc.validateMenuItem(try item(m, "Layers > Move Layer to Bottom")))
        wc.moveLayerToBottom(nil)
        XCTAssertTrue(waitUntil { e.info.layers[0].name == top }, e.message ?? "")
        let active = e.info.activeLayer
        XCTAssertEqual(e.info.layers[active].name, top, "the moved layer stays active")
        wc.moveLayerToTop(nil)
        XCTAssertTrue(waitUntil { e.info.layers[1].name == top }, e.message ?? "")
    }

    // MARK: Save All

    func testSaveAllSavesEveryEditedImageAndIsOffWhenNothingIsEdited() async throws {
        let dir = try ImageIOFixtures.directory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let savedEngine = ImageDocument.engine, savedWorkspace = ImageDocument.workspace
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        var docs: [ImageDocument] = []
        defer {
            for d in docs {
                for wc in d.windowControllers { wc.window?.orderOut(nil) }
                d.close()
            }
            ImageDocument.engine = savedEngine
            ImageDocument.workspace = savedWorkspace
        }
        for name in ["a.png", "b.png", "c.png"] {
            let url = dir.appendingPathComponent(name)
            try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
            let doc = try ImageDocument(contentsOf: url, ofType: "public.png")
            docs.append(doc)
            doc.makeWindowControllers()
            for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
            doc.showWindows()
        }
        XCTAssertFalse(ImageDocumentController.canSaveAll(docs), "nothing edited: Save All is off")
        for d in docs.prefix(2) {
            let status = try await XCTUnwrap(d.editor).runAndWait("image.invert_colors", [:])
            XCTAssertEqual(status, "ok")
            try XCTUnwrap(d.editor).refresh()
        }
        XCTAssertTrue(waitUntil { docs[0].isDocumentEdited && docs[1].isDocumentEdited })
        XCTAssertTrue(ImageDocumentController.canSaveAll(docs))
        let saved = ImageDocumentController.saveAll(docs)
        XCTAssertEqual(saved.count, 2, "only the edited images")
        XCTAssertTrue(waitUntil(20) { !docs[0].isDocumentEdited && !docs[1].isDocumentEdited })
        for d in docs.prefix(2) {
            let back = try ImageIOImporter.decode(url: try XCTUnwrap(d.fileURL))
            XCTAssertEqual(back.rgba, try XCTUnwrap(d.engineDocument).pixels(), "\(d.displayName) holds the edit")
        }
        XCTAssertFalse(ImageDocumentController.canSaveAll(docs))
        // The menu item calls the document controller's Save All.
        XCTAssertEqual(try item(menu(), "File > Save All").action, #selector(NSDocumentController.saveAllDocuments(_:)))
    }

    // MARK: Repeat

    /// Cmd+F repeats each seeded effect with the same values and seed: the same
    /// bytes again, and the item names the effect and shows its icon.
    func testRepeatGivesIdenticalBytesForEverySeededEffect() throws {
        let wc = try fx.open()
        let e = wc.editor
        let repeatItem = try item(menu(), "Effects > Repeat Effect")
        XCTAssertEqual(repeatItem.keyEquivalent, "f")
        XCTAssertEqual(repeatItem.keyEquivalentModifierMask, [.command])
        let seeded = try EffectCatalog.all(fx.engine).filter { $0.isSeeded }
        XCTAssertEqual(Set(seeded.map(\.id)), ["effect.noise.add", "effect.distort.crystalize", "effect.render.clouds",
                                                "effect.distort.dents", "effect.distort.frosted_glass",
                                                "effect.render.turbulence"])
        for s in seeded {
            let steps = e.info.historyLabels.count
            let d = try XCTUnwrap(wc.showEffect(s.id), s.id)
            d.reseed(nil)
            XCTAssertTrue(waitUntil(30) { d.isIdle }, s.id)
            d.ok(nil)
            XCTAssertTrue(waitUntil(60) { e.info.historyLabels.count == steps + 1 }, "\(s.id): \(e.message ?? "")")
            let once = try e.document.pixels()
            e.run("edit.undo")
            XCTAssertTrue(waitUntil { e.info.historyIndex == steps - 1 })
            XCTAssertTrue(wc.validateMenuItem(repeatItem), s.id)
            XCTAssertEqual(repeatItem.title, "Repeat \(s.label)")
            XCTAssertNotNil(repeatItem.image, "Repeat shows \(s.label)'s icon")
            XCTAssertNotNil(MenuIcons.effectSymbols[s.id], "\(s.id) has its own icon")
            wc.repeatEffect(repeatItem)
            XCTAssertNil(wc.window?.attachedSheet, "no dialog")
            XCTAssertTrue(waitUntil(60) { e.info.historyLabels.count == steps + 1 && e.info.historyIndex == steps },
                          s.id)
            XCTAssertEqual(try e.document.pixels(), once, "\(s.id): same parameters and seed, same bytes")
        }
    }
}

// MARK: Render (visual check)

extension MenuCatalogTests {
    /// With TEST_RUNNER_M55_PNG_DIR set, draws every menu's items (icon, title,
    /// shortcut, off items dimmed) into menus-light.png and menus-dark.png. The
    /// menu bar itself cannot be drawn off screen, so this sheet stands in for it.
    func testRenderMenuSheets() throws {
        guard let dir = ProcessInfo.processInfo.environment["M55_PNG_DIR"] else {
            throw XCTSkip("set M55_PNG_DIR to render the menu sheets")
        }
        let wc = try fx.open()
        let m = menu()
        let columns = ["File", "Edit", "View", "Image", "Layers", "Adjustments", "Effects"]
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let sheet = NSStackView()
            sheet.orientation = .horizontal
            sheet.alignment = .top
            sheet.spacing = 12
            sheet.edgeInsets = NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12)
            for title in columns {
                let col = NSStackView()
                col.orientation = .vertical
                col.alignment = .leading
                col.spacing = 2
                let head = NSTextField(labelWithString: title)
                head.font = .boldSystemFont(ofSize: 13)
                col.addArrangedSubview(head)
                func add(_ menu: NSMenu, indent: CGFloat) {
                    for item in menu.items where !item.isHidden {
                        if item.isSeparatorItem {
                            let line = NSBox()
                            line.boxType = .separator
                            line.widthAnchor.constraint(equalToConstant: 200).isActive = true
                            col.addArrangedSubview(line)
                            continue
                        }
                        if title == "View", menu.title == "Zoom Presets" { continue }
                        let row = NSStackView()
                        row.orientation = .horizontal
                        row.spacing = 6
                        let pad = NSView()
                        pad.widthAnchor.constraint(equalToConstant: indent).isActive = true
                        row.addArrangedSubview(pad)
                        let icon = NSImageView()
                        icon.image = item.image
                        icon.contentTintColor = .labelColor
                        icon.widthAnchor.constraint(equalToConstant: 16).isActive = true
                        icon.heightAnchor.constraint(equalToConstant: 16).isActive = true
                        row.addArrangedSubview(icon)
                        let enabled = item.submenu != nil || (item.action != nil && (wc.validateMenuItem(item) || item.target != nil))
                        let label = NSTextField(labelWithString: item.title + (item.submenu != nil ? "  ▸" : ""))
                        label.font = .menuFont(ofSize: 13)
                        label.textColor = enabled ? .labelColor : .tertiaryLabelColor
                        label.widthAnchor.constraint(equalToConstant: 190 - indent).isActive = true
                        row.addArrangedSubview(label)
                        let key = NSTextField(labelWithString: item.keyEquivalent.isEmpty ? "" : MenuSkeleton.shortcut(item))
                        key.font = .menuFont(ofSize: 12)
                        key.textColor = .secondaryLabelColor
                        key.widthAnchor.constraint(equalToConstant: 80).isActive = true
                        row.addArrangedSubview(key)
                        col.addArrangedSubview(row)
                        if let sub = item.submenu { add(sub, indent: indent + 14) }
                    }
                }
                add(try XCTUnwrap(m.item(withTitle: title)?.submenu), indent: 0)
                sheet.addArrangedSubview(col)
            }
            let root = NSView()
            root.appearance = NSAppearance(named: appearance)
            root.wantsLayer = true
            sheet.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(sheet)
            NSLayoutConstraint.activate([sheet.leadingAnchor.constraint(equalTo: root.leadingAnchor),
                                         sheet.topAnchor.constraint(equalTo: root.topAnchor)])
            root.frame = NSRect(origin: .zero, size: sheet.fittingSize)
            root.layoutSubtreeIfNeeded()
            root.appearance?.performAsCurrentDrawingAppearance {
                root.layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
            }
            let rep = try XCTUnwrap(root.bitmapImageRepForCachingDisplay(in: root.bounds))
            root.cacheDisplay(in: root.bounds, to: rep)
            let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
            try png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("menus-\(name).png"))
        }
    }
}
