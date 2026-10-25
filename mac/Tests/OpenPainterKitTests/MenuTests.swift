// The menu skeleton: order and items match the checked-in list
// (menu-skeleton.txt), the effect menus mirror the engine registry, and every
// registered adjustment runs with its defaults.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class MenuTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    func testMenuSkeletonOrderAndItemsMatchTheCheckedInList() throws {
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: fx.workspace.menuTarget)
        let lines = MenuSkeleton.describe(menu)
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("menu-skeleton.txt")
        let expected = try String(contentsOf: url, encoding: .utf8)
            .split(separator: "\n", omittingEmptySubsequences: true)
            .map(String.init)
            .filter { !$0.hasPrefix("#") }
        if lines != expected {
            print("MENU SKELETON (actual)\n" + lines.joined(separator: "\n"))
        }
        XCTAssertEqual(lines.count, expected.count, "item count")
        XCTAssertEqual(lines, expected)
        XCTAssertEqual(menu.items.map(\.title),
                       ["OpenPainter", "File", "Edit", "View", "Image", "Layers", "Adjustments", "Effects", "Window", "Help"])
    }

    func testEffectMenusMirrorTheRegistry() throws {
        let registry = try fx.engine.effects()
        XCTAssertFalse(registry.isEmpty)
        let menu = MenuSkeleton.build(effects: registry, panelTarget: nil)
        func effectItems(_ m: NSMenu) -> [NSMenuItem] {
            m.items.flatMap { item -> [NSMenuItem] in
                if let sub = item.submenu { return effectItems(sub) }
                return item.action == #selector(EditorMenuActions.runEffect(_:)) ? [item] : []
            }
        }
        let adjustments = try XCTUnwrap(menu.item(withTitle: "Adjustments")?.submenu)
        let adj = effectItems(adjustments)
        let registeredAdjustments = registry.filter { $0.category == "Adjustments" }
        XCTAssertEqual(adj.map { $0.representedObject as? String }, registeredAdjustments.map(\.id), "registry order")
        XCTAssertEqual(adj.map(\.title), registeredAdjustments.map(\.label))
        // Every registered effect appears exactly once across the menu bar.
        let all = effectItems(menu).compactMap { $0.representedObject as? String }
        XCTAssertEqual(all.sorted(), registry.map(\.id).sorted())
        // No submenu lists the same name twice (a registered effect and its disabled placeholder).
        func checkUnique(_ m: NSMenu) {
            let titles = m.items.filter { !$0.isSeparatorItem }.map(\.title)
            XCTAssertEqual(titles.count, Set(titles).count, "duplicate items in \(m.title)")
            m.items.compactMap(\.submenu).forEach(checkUnique)
        }
        checkUnique(menu)
        // paint.net adjustments the engine does not have yet are present and disabled.
        let missing = MenuSkeleton.paintNetAdjustments.map(\.0).filter { t in !registeredAdjustments.contains { $0.label == t } }
        for title in missing {
            let item = try XCTUnwrap(adjustments.item(withTitle: title), title)
            XCTAssertNil(item.action, "\(title) is disabled")
        }
        XCTAssertEqual(adjustments.item(withTitle: "Invert Colors")?.keyEquivalent, "i")
        XCTAssertEqual(adjustments.item(withTitle: "Invert Colors")?.keyEquivalentModifierMask, [.command, .shift])
        // A registry with an added effect gets a menu item for it, in its submenu.
        // The stand-in is the first paint.net effect the engine does not have yet,
        // so new engine effects never collide with it.
        let (submenu, title) = try XCTUnwrap(
            MenuSkeleton.paintNetEffects.lazy
                .flatMap { sub in sub.1.map { (sub.0, $0) } }
                .first { pair in !registry.contains { $0.label == pair.1 } },
            "every paint.net effect is registered; pick another stand-in")
        let id = "effect.test.stand_in"
        let extra = registry + [Engine.EffectInfo(id: id, label: title, category: submenu)]
        let sub = MenuSkeleton.build(effects: extra, panelTarget: nil).item(withTitle: "Effects")?.submenu?
            .item(withTitle: submenu)?.submenu
        let item = try XCTUnwrap(sub?.item(withTitle: title))
        XCTAssertEqual(item.representedObject as? String, id)
        XCTAssertEqual(item.action, #selector(EditorMenuActions.runEffect(_:)))
        XCTAssertEqual(sub?.items.filter { $0.title == title }.count, 1, "no disabled twin")
    }

    func testEveryAdjustmentRunsWithDefaultsFromItsMenuItem() throws {
        let wc = try fx.open()
        let editor = wc.editor
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let adjustments = try XCTUnwrap(menu.item(withTitle: "Adjustments")?.submenu)
        let items = adjustments.items.filter { $0.action == #selector(EditorMenuActions.runEffect(_:)) }
        XCTAssertFalse(items.isEmpty)
        for (n, item) in items.enumerated() {
            XCTAssertTrue(wc.validateMenuItem(item), "\(item.title) enabled")
            wc.runEffect(item)
            XCTAssertTrue(waitUntil { editor.info.historyLabels.count == n + 2 }, "\(item.title) ran: \(editor.message ?? "")")
            XCTAssertEqual(editor.info.historyLabels.last, item.title)
        }
        XCTAssertTrue(editor.info.dirty)
        XCTAssertTrue(waitUntil { wc.window?.isDocumentEdited == true }, "the window shows Edited")
    }

    func testEngineCommandItemsFollowTheEngineState() throws {
        let wc = try fx.open()
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: nil)
        let edit = try XCTUnwrap(menu.item(withTitle: "Edit")?.submenu)
        let image = try XCTUnwrap(menu.item(withTitle: "Image")?.submenu)
        let deselect = try XCTUnwrap(edit.item(withTitle: "Deselect"))
        let crop = try XCTUnwrap(image.item(withTitle: "Crop to Selection"))
        XCTAssertFalse(wc.validateMenuItem(deselect))
        XCTAssertFalse(wc.validateMenuItem(crop))
        XCTAssertFalse(wc.commandButtons["deselect"]!.isEnabled)
        wc.selectAllPixels()
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        XCTAssertTrue(wc.validateMenuItem(deselect))
        XCTAssertTrue(wc.validateMenuItem(crop))
        XCTAssertTrue(wc.commandButtons["deselect"]!.isEnabled)
        XCTAssertTrue(wc.commandButtons["crop"]!.isEnabled)
        wc.runCommand(deselect)
        XCTAssertTrue(waitUntil { wc.editor.selection == nil })
        // Not-yet-built toolbar commands keep their place, disabled.
        for k in ["new", "save", "print", "cut", "copy", "paste", "rulers"] {
            XCTAssertFalse(wc.commandButtons[k]!.isEnabled, k)
        }
        XCTAssertTrue(wc.commandButtons["open"]!.isEnabled)
        // Layers > Add New Layer and Go to Layer use the engine.
        let layers = try XCTUnwrap(menu.item(withTitle: "Layers")?.submenu)
        wc.addLayer(layers.item(withTitle: "Add New Layer"))
        XCTAssertTrue(waitUntil { wc.editor.info.layers.count == 2 })
        let top = try XCTUnwrap(layers.item(withTitle: "Go to Top Layer"))
        XCTAssertTrue(wc.validateMenuItem(top))
        wc.goToLayer(top)
        XCTAssertTrue(waitUntil { wc.editor.info.activeLayer == 1 })
        XCTAssertFalse(wc.validateMenuItem(top), "already on top")
    }
}
