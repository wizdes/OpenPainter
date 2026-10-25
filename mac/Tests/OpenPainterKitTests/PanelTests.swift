// Floating panels: toggles equal visibility for every panel and both shortcut
// sets, saved positions come back, a missing display brings panels back on
// screen, panels never overlap, and the fit rectangle leaves them out.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class PanelTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    /// The SPEC's 1280 x 800 example: the stage starts below 28 menu + 6 margin +
    /// 52 toolbar + 36 options + 64 strip and is 582 pt high.
    let stage = CGRect(x: 100, y: 100, width: 1268, height: 582)

    func testDefaultPlacesMatchTheSpec() {
        let d = PanelLayout.defaults(stage: stage)
        let tools = d[.tools]!, colors = d[.colors]!, history = d[.history]!, layers = d[.layers]!
        // Distances from the stage's top edge (y grows up in screen space).
        XCTAssertEqual(stage.maxY - tools.maxY, 12)
        XCTAssertEqual(stage.maxY - tools.minY, 304, "Tools ends at stage y=304")
        XCTAssertEqual(stage.maxY - colors.maxY, 324, "Colors begins at y=324")
        XCTAssertEqual(stage.maxY - history.minY, 257, "History ends at y=257")
        // The SPEC text says Layers begins at y=324, but its table gives Layers a
        // 245 pt height: 582 - 12 - 245 = 325. The table wins here.
        XCTAssertEqual(stage.maxY - layers.maxY, 325, "Layers begins at y=325")
        XCTAssertEqual(tools.minX - stage.minX, 12)
        XCTAssertEqual(stage.maxX - layers.maxX, 12)
        XCTAssertTrue(PanelLayout.noOverlap(Array(d.values), gap: 12))
        let clear = PanelLayout.clearRect(stage: stage, panels: Array(d.values))
        XCTAssertEqual(clear.minX - stage.minX, 272, "left reserve")
        XCTAssertEqual(stage.maxX - clear.maxX, 254, "right reserve")
        for p in d.values { XCTAssertFalse(clear.intersects(p)) }
        // At the minimum window height the left stack exactly fills the stage.
        let short = CGRect(x: 0, y: 0, width: 1100, height: 574)
        let s = PanelLayout.defaults(stage: short)
        XCTAssertEqual(s[.tools]!.height, 292)
        XCTAssertEqual(s[.colors]!.minY, 12)
        XCTAssertEqual(s[.tools]!.maxY, 562)
        // Shorter still: Tools shrinks (and scrolls), never below its minimum.
        let tiny = PanelLayout.defaults(stage: CGRect(x: 0, y: 0, width: 1100, height: 450))
        XCTAssertEqual(tiny[.tools]!.height, 450 - 36 - 246)
        XCTAssertEqual(tiny[.history]!.height, 450 - 36 - 245)
        XCTAssertTrue(PanelLayout.noOverlap(Array(tiny.values), gap: 12))
    }

    func testToggleStateEqualsVisibilityForEachPanelAndBothShortcutSets() throws {
        let wc = try fx.open()
        let panels = fx.workspace.panels
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: fx.workspace.menuTarget)
        let windowMenu = try XCTUnwrap(menu.item(withTitle: "Window")?.submenu)
        for k in PanelKind.allCases {
            XCTAssertTrue(panels.isShown(k), "\(k) starts shown")
            XCTAssertTrue(panels.panels[k]!.isVisible)
        }
        func check(_ k: PanelKind, _ shown: Bool, _ how: String) {
            XCTAssertEqual(panels.isShown(k), shown, "\(k) \(how)")
            XCTAssertEqual(panels.panels[k]!.isVisible, shown, "\(k) window \(how)")
            XCTAssertEqual(wc.panelToggles[k]!.isOn, shown, "\(k) toolbar toggle \(how)")
            let item = windowMenu.items.first { $0.title == k.title && !$0.isHidden }!
            _ = fx.workspace.menuTarget.validateMenuItem(item)
            XCTAssertEqual(item.state == .on, shown, "\(k) menu check mark \(how)")
        }
        for k in PanelKind.allCases {
            let f = keyEvent(functionKey(k.functionKey), keyCode: functionKeyCodes[k.functionKey]!, modifiers: [.function])
            XCTAssertTrue(menu.performKeyEquivalent(with: f), "F\(k.functionKey)")
            check(k, false, "after F\(k.functionKey)")
            XCTAssertTrue(menu.performKeyEquivalent(with: f))
            check(k, true, "after F\(k.functionKey) twice")
            let d = keyEvent("\(k.digit)", keyCode: digitKeyCodes[k.digit]!, modifiers: [.command, .option])
            XCTAssertTrue(menu.performKeyEquivalent(with: d), "⌥⌘\(k.digit)")
            check(k, false, "after ⌥⌘\(k.digit)")
            // The toolbar toggle and the panel's own close button agree too.
            wc.panelToggles[k]!.performClick(nil)
            check(k, true, "after the toolbar toggle")
            _ = panels.windowShouldClose(panels.panels[k]!)
            check(k, false, "after the close button")
            XCTAssertTrue(menu.performKeyEquivalent(with: d))
            check(k, true, "after ⌥⌘\(k.digit) twice")
        }
    }

    func testPositionsAreSavedAndRestored() throws {
        let screens = [CGRect(x: 0, y: 0, width: 1440, height: 900)]
        let panels = fx.workspace.panels
        panels.screens = { screens }
        panels.screenFrames = { screens }
        panels.stage = { CGRect(x: 0, y: 100, width: 1268, height: 582) }
        panels.showAll()
        let moved = CGRect(x: 400, y: 300, width: 84, height: panels.frame(.tools).height)
        panels.panels[.tools]!.setFrame(moved, display: false)
        panels.userMoved(.tools)
        panels.setShown(.history, false)
        let saved = PanelKind.allCases.map(panels.frame)

        let again = PanelManager(store: PanelStore(defaults: fx.defaults), hidesOnDeactivate: false)
        again.screens = { screens }
        again.screenFrames = { screens }
        again.stage = { CGRect(x: 0, y: 0, width: 10, height: 10) }  // ignored: a record exists
        XCTAssertTrue(again.restore())
        XCTAssertEqual(PanelKind.allCases.map(again.frame), saved)
        XCTAssertEqual(again.frame(.tools), moved)
        XCTAssertFalse(again.isShown(.history))
        XCTAssertTrue(again.isShown(.layers))
        // A second arrangement keeps its own record.
        let two = screens + [CGRect(x: 1440, y: 0, width: 1920, height: 1080)]
        again.screens = { two }
        again.screenFrames = { two }
        again.panels[.colors]!.setFrame(CGRect(x: 2000, y: 200, width: 244, height: 246), display: false)
        again.userMoved(.colors)
        XCTAssertEqual(again.frame(.colors).minX, 2000)
        again.screens = { screens }
        again.screenFrames = { screens }
        again.screensChanged()
        XCTAssertEqual(again.frame(.tools), moved, "back on one screen: its saved layout")
        XCTAssertLessThan(again.frame(.colors).maxX, 1440)
        for p in again.panels.values { p.close() }
    }

    func testMissingDisplayMovesPanelsBackOnScreen() throws {
        let laptop = CGRect(x: 0, y: 0, width: 1440, height: 875)
        let external = CGRect(x: 1440, y: -200, width: 2560, height: 1415)
        let panels = fx.workspace.panels
        panels.screens = { [laptop, external] }
        panels.screenFrames = { [laptop, external] }
        panels.stage = { CGRect(x: 1500, y: 100, width: 1268, height: 582) }  // the window is on the external display
        // Placed but not ordered front: AppKit would pull a shown panel onto this
        // Mac's real screen, and the external display here is simulated.
        panels.placeIfNeeded()
        for k in PanelKind.allCases { XCTAssertGreaterThan(panels.frame(k).minX, 1440, "\(k) on the external display") }

        // Next launch: the external display is gone, and only the laptop is left.
        let again = PanelManager(store: PanelStore(defaults: fx.defaults), hidesOnDeactivate: false)
        again.screens = { [laptop] }
        again.screenFrames = { [laptop] }
        XCTAssertTrue(again.restore())
        for k in PanelKind.allCases {
            let f = again.frame(k)
            XCTAssertTrue(laptop.contains(f), "\(k) at \(f) is on the laptop screen")
            XCTAssertTrue(PanelLayout.isReachable(f, screens: [laptop]))
        }
        XCTAssertTrue(PanelLayout.noOverlap(PanelKind.allCases.map(again.frame)), "moved panels do not overlap")
        // The same while running: the display disappears under the open panels.
        panels.screens = { [laptop] }
        panels.screenFrames = { [laptop] }
        panels.screensChanged()
        for k in PanelKind.allCases { XCTAssertTrue(laptop.contains(panels.frame(k)), "\(k) moved on screen") }
        XCTAssertTrue(PanelLayout.noOverlap(PanelKind.allCases.map(panels.frame)))
        for p in again.panels.values { p.close() }
    }

    func testPanelsNeverOverlapAndTheFitRectangleExcludesThem() throws {
        let wc = try fx.open()
        let panels = fx.workspace.panels
        let all = NSScreen.screens.map(\.visibleFrame)
        panels.screens = { all }
        // Drop each panel on top of another one: it moves clear, 12 pt apart.
        for (k, onto) in [(PanelKind.colors, PanelKind.tools), (.layers, .history), (.history, .colors), (.tools, .layers)] {
            let target = panels.frame(onto)
            let size = panels.frame(k).size
            panels.panels[k]!.setFrame(CGRect(origin: CGPoint(x: target.minX + 10, y: target.maxY - size.height - 5), size: size), display: false)
            panels.userMoved(k)
            XCTAssertTrue(PanelLayout.noOverlap(PanelKind.allCases.map(panels.frame), gap: 11.9),
                          "after dropping \(k) on \(onto): \(PanelKind.allCases.map(panels.frame))")
        }
        // The canvas's clear rectangle never covers a shown panel.
        panels.resetLayout()
        wc.updateClearRect()
        let canvas = wc.canvas
        let clear = try XCTUnwrap(canvas.viewport.clearRect)
        let clearScreen = wc.window!.convertToScreen(canvas.convert(clear, to: nil))
        for f in panels.shownFrames { XCTAssertFalse(clearScreen.intersects(f), "clear area \(clearScreen) vs panel \(f)") }
        // The fitted image sits inside the clear rectangle.
        canvas.fit()
        let v = canvas.viewport
        let image = CGRect(origin: v.origin, size: v.contentSize)
        XCTAssertTrue(clear.insetBy(dx: -1, dy: -1).contains(image), "image \(image) inside \(clear)")
        // Hiding the left panels widens the clear area and the fit follows.
        let before = canvas.viewport.clearRect!.width
        panels.setShown(.tools, false)
        panels.setShown(.colors, false)
        XCTAssertGreaterThan(canvas.viewport.clearRect!.width, before)
        // A chosen zoom is kept when a panel opens.
        canvas.actualSize(nil)
        panels.setShown(.colors, true)
        XCTAssertEqual(canvas.viewport.zoom, 1)
    }

    func testResetPanelLayoutRestoresDefaultsAndShowsAll() throws {
        let wc = try fx.open()
        let panels = fx.workspace.panels
        panels.setShown(.layers, false)
        panels.panels[.tools]!.setFrame(panels.frame(.tools).offsetBy(dx: 300, dy: -100), display: false)
        panels.userMoved(.tools)
        fx.workspace.menuTarget.resetPanelLayout(nil)
        let d = PanelLayout.defaults(stage: wc.stageInScreen!, sizes: panels.defaultSizes)
        for k in PanelKind.allCases {
            XCTAssertEqual(panels.frame(k), d[k], "\(k)")
            XCTAssertTrue(panels.isShown(k))
        }
        // The Tools panel is exactly as tall as its tool list: no empty space.
        let content = panels.panels[.tools]!.contentRect(forFrameRect: panels.frame(.tools))
        XCTAssertEqual(content.height, ToolsPanelView.fittingHeight)
    }
}
