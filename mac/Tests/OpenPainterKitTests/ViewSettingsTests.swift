// M56: the View menu (zoom presets, rulers, units, pixel grid), the Settings
// window, and light and dark appearance. Ruler ticks and unit conversions are
// checked against numbers worked out by hand at 72, 96 and 300 DPI.
import AppKit
@testable import OpenPainterKit
import XCTest

final class UnitAndRulerMathTests: XCTestCase {
    func testUnitConversionsAt72_96And300DPI() {
        let cases: [(dpi: Double, px: Double, inches: Double, cm: Double)] = [
            (72, 72, 1, 2.54),
            (72, 36, 0.5, 1.27),
            (96, 96, 1, 2.54),
            (96, 512, 5.333_333_333, 13.546_666_667),
            (300, 300, 1, 2.54),
            (300, 118.110_236_22, 0.393_700_787, 1),
            (300, 6000, 20, 50.8),
        ]
        for c in cases {
            XCTAssertEqual(MeasurementUnit.pixels.value(ofPixels: c.px, dpi: c.dpi), c.px, accuracy: 1e-9)
            XCTAssertEqual(MeasurementUnit.inches.value(ofPixels: c.px, dpi: c.dpi), c.inches, accuracy: 1e-8, "\(c)")
            XCTAssertEqual(MeasurementUnit.centimeters.value(ofPixels: c.px, dpi: c.dpi), c.cm, accuracy: 1e-8, "\(c)")
            XCTAssertEqual(MeasurementUnit.inches.pixels(ofValue: c.inches, dpi: c.dpi), c.px, accuracy: 1e-6)
            XCTAssertEqual(MeasurementUnit.centimeters.pixels(ofValue: c.cm, dpi: c.dpi), c.px, accuracy: 1e-6)
        }
        XCTAssertEqual(MeasurementUnit.centimeters.pixelsPerUnit(dpi: 254), 100, accuracy: 1e-12)
        XCTAssertEqual(MeasurementUnit.inches.format(5.333_333), "5.33")
        XCTAssertEqual(MeasurementUnit.pixels.format(511.9), "511")
        XCTAssertEqual(MeasurementUnit.allCases.map(\.abbreviation), ["px", "in", "cm"])
    }

    /// Each row: the unit, DPI and zoom, then the expected major step (units),
    /// minor parts and points per unit, worked out by hand from
    /// pointsPerUnit = zoom × pixels per unit and the 48 pt / 4 pt spacing rule.
    func testTickSpacingAtSeveralZoomsAndResolutions() {
        let rows: [(MeasurementUnit, Double, Double, Double, Int, Double)] = [
            (.pixels, 96, 1, 50, 5, 1),  // 50 px = 50 pt; 10 px minors
            (.pixels, 96, 4, 20, 4, 4),  // 20 px = 80 pt (10 px = 40 pt is too close); 5 px minors
            (.pixels, 96, 0.25, 200, 4, 0.25),  // 200 px = 50 pt; 50 px minors
            (.pixels, 300, 32, 2, 2, 32),  // 2 px = 64 pt; 1 px minors (whole pixels only)
            (.inches, 72, 1, 1, 10, 72),  // 1 in = 72 pt; 0.1 in = 7.2 pt
            (.inches, 72, 4, 0.2, 4, 288),  // 0.2 in = 57.6 pt; 0.05 in = 14.4 pt
            (.inches, 96, 1, 0.5, 5, 96),  // 0.5 in = 48 pt; 0.1 in = 9.6 pt
            (.inches, 300, 1, 0.2, 4, 300),  // 0.2 in = 60 pt; 0.05 in = 15 pt
            (.inches, 300, 0.1, 2, 4, 30),  // 2 in = 60 pt; 0.5 in = 15 pt
            (.centimeters, 72, 1, 2, 4, 72 / 2.54),  // 2 cm = 56.69 pt; 0.5 cm = 14.17 pt
            (.centimeters, 96, 2, 1, 10, 192 / 2.54),  // 0.5 cm = 37.8 pt is too close; 1 cm = 75.6 pt, 0.1 cm = 7.6 pt
            (.centimeters, 300, 0.25, 2, 4, 75 / 2.54),  // 2 cm = 59.06 pt; 0.5 cm = 14.76 pt
        ]
        for (i, r) in rows.enumerated() {
            let s = RulerScale(unit: r.0, dpi: r.1, zoom: r.2)
            XCTAssertEqual(s.pointsPerUnit, r.5, accuracy: 1e-9, "row \(i) points per unit")
            XCTAssertEqual(s.majorStep, r.3, accuracy: 1e-12, "row \(i) major step")
            XCTAssertEqual(s.subdivisions, r.4, "row \(i) minor parts")
            XCTAssertGreaterThanOrEqual(s.majorStep * s.pointsPerUnit, 48 - 1e-9, "row \(i)")
            XCTAssertGreaterThanOrEqual(s.minorStep * s.pointsPerUnit, 4 - 1e-9, "row \(i)")
        }
    }

    func testTickPositionsInPixelsAtZoom1() {
        let s = RulerScale(unit: .pixels, dpi: 96, zoom: 1)
        let ticks = s.ticks(origin: 10, length: 200)
        // Minors every 10 px from x = 0 (value -10) to x = 200 (value 190).
        XCTAssertEqual(ticks.map(\.position), stride(from: 0.0, through: 200, by: 10).map { $0 })
        XCTAssertEqual(ticks.map(\.value), stride(from: -10.0, through: 190, by: 10).map { $0 })
        XCTAssertEqual(ticks.filter { $0.level == .major }.map(\.position), [10, 60, 110, 160])
        XCTAssertEqual(ticks.filter { $0.level == .major }.map { s.label($0.value) }, ["0", "50", "100", "150"])
        XCTAssertEqual(ticks.filter { $0.level == .middle }.count, 0, "5 parts have no middle tick")
    }

    func testTickPositionsInInchesAt300DPIAnd96DPI() {
        // 300 DPI at 50%: 150 pt per inch, so the step is 0.5 in = 75 pt with
        // 5 minors of 0.1 in = 15 pt. The document starts at x = -20 (scrolled).
        let s = RulerScale(unit: .inches, dpi: 300, zoom: 0.5)
        XCTAssertEqual(s.pointsPerUnit, 150)
        XCTAssertEqual(s.majorStep, 0.5)
        XCTAssertEqual(s.subdivisions, 5)
        let ticks = s.ticks(origin: -20, length: 160)
        let majors = ticks.filter { $0.level == .major }
        // Values 0.5 and 1.0 in sit at -20 + 75 = 55 and -20 + 150 = 130.
        XCTAssertEqual(majors.map(\.value), [0.5, 1.0])
        XCTAssertEqual(majors.map(\.position)[0], 55, accuracy: 1e-9)
        XCTAssertEqual(majors.map(\.position)[1], 130, accuracy: 1e-9)
        XCTAssertEqual(majors.map { s.label($0.value) }, ["0.5", "1.0"])
        // The first tick is 0.2 in (x = 10); 0.1 in would be at -5.
        XCTAssertEqual(ticks.first?.position ?? -1, 10, accuracy: 1e-9)
        XCTAssertEqual(ticks.first?.value ?? -1, 0.2, accuracy: 1e-12)
        for t in ticks { XCTAssertEqual(t.position, -20 + t.value * 150, accuracy: 1e-9) }
        // 96 DPI at 200%: 192 pt per inch; step 0.5 in = 96 pt, 0.1 in minors.
        let s96 = RulerScale(unit: .inches, dpi: 96, zoom: 2)
        XCTAssertEqual(s96.majorStep, 0.5)
        XCTAssertEqual(s96.subdivisions, 5)
        let t96 = s96.ticks(origin: 0, length: 400)
        XCTAssertEqual(t96.filter { $0.level == .major }.map(\.position), [0, 96, 192, 288, 384])
        XCTAssertEqual(t96.count, 21, "0.1 in = 19.2 pt apart from 0 to 384")
    }

    func testTickPositionsInCentimetersAt72DPIWithMiddleTicks() {
        // 72 DPI at 100%: 28.3465 pt per cm. Step 2 cm (56.69 pt), 4 parts of
        // 0.5 cm; every second minor (each whole cm) is a middle tick.
        let s = RulerScale(unit: .centimeters, dpi: 72, zoom: 1)
        let ppc = 72 / 2.54
        let ticks = s.ticks(origin: 0, length: 120)
        XCTAssertEqual(ticks.count, 9, "0, 0.5, ... 4.0 cm; 4.5 cm is at 127.6 pt")
        for (k, t) in ticks.enumerated() {
            XCTAssertEqual(t.value, Double(k) * 0.5, accuracy: 1e-12)
            XCTAssertEqual(t.position, Double(k) * 0.5 * ppc, accuracy: 1e-9)
            let want: RulerTick.Level = k % 4 == 0 ? .major : (k % 2 == 0 ? .middle : .minor)
            XCTAssertEqual(t.level, want, "tick \(k)")
        }
        XCTAssertEqual(s.label(2), "2")
    }

    func testBadResolutionFallsBackTo96() {
        XCTAssertEqual(RulerScale(unit: .inches, dpi: 0, zoom: 1).pointsPerUnit, 96)
        XCTAssertEqual(RulerScale(unit: .inches, dpi: .nan, zoom: 1).pointsPerUnit, 96)
        XCTAssertTrue(RulerScale(unit: .pixels, dpi: 96, zoom: 1).ticks(origin: 0, length: 0).isEmpty)
    }
}

@MainActor
final class ViewSettingsTests: XCTestCase {
    var fx: ShellFixture!
    var appearanceBefore: NSAppearance?

    override func setUp() async throws {
        appearanceBefore = NSApp?.appearance
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        SettingsWindowController.shared?.close()
        SettingsWindowController.shared = nil
        fx.tearDown()
        NSApp.appearance = appearanceBefore
    }

    var settings: AppSettings { fx.workspace.settings }

    func viewMenu() throws -> NSMenu {
        let menu = MenuSkeleton.build(effects: fx.workspace.effects, panelTarget: fx.workspace.menuTarget)
        return try XCTUnwrap(menu.item(withTitle: "View")?.submenu)
    }

    func testRulersFollowZoomScrollUnitsAndResolution() throws {
        let wc = try fx.open()
        let c = wc.container
        XCTAssertFalse(c.rulersVisible, "rulers are off by default")
        XCTAssertEqual(c.canvas.frame.origin, .zero)
        settings.rulers = true
        XCTAssertTrue(c.rulersVisible)
        XCTAssertEqual(c.canvas.frame.minX, RulerView.thickness)
        XCTAssertEqual(c.canvas.frame.minY, RulerView.thickness)
        XCTAssertEqual(c.horizontalRuler.frame.width, c.canvas.frame.width)
        XCTAssertEqual(c.verticalRuler.frame.height, c.canvas.frame.height)
        XCTAssertFalse(c.horizontalRuler.isHidden)
        XCTAssertTrue(wc.commandButtons["rulers"]!.isEnabled)
        XCTAssertTrue((wc.commandButtons["rulers"] as? ToggleButton)?.isOn == true)
        for zoom in [0.25, 1, 4] {
            var v = c.canvas.viewport
            v.setZoom(zoom)
            c.canvas.setViewport(v)
            v = c.canvas.viewport
            XCTAssertEqual(c.horizontalRuler.scale.zoom, zoom)
            XCTAssertEqual(c.horizontalRuler.origin, v.origin.x)
            XCTAssertEqual(c.verticalRuler.origin, v.origin.y)
            // Document pixel 100 is at its view position on both rulers.
            let p = v.viewPoint(ofDocument: CGPoint(x: 100, y: 100))
            XCTAssertEqual(c.horizontalRuler.scale.position(ofValue: 100, origin: Double(v.origin.x)), Double(p.x), accuracy: 1e-9)
            XCTAssertEqual(c.verticalRuler.scale.position(ofValue: 100, origin: Double(v.origin.y)), Double(p.y), accuracy: 1e-9)
            for t in c.horizontalRuler.ticks {
                XCTAssertEqual(t.position, Double(v.origin.x) + t.value * zoom, accuracy: 1e-9)
            }
        }
        // Scrolling moves the ticks with the document.
        var v = c.canvas.viewport
        v.setZoom(8)
        c.canvas.setViewport(v)
        let before = c.horizontalRuler.origin
        v = c.canvas.viewport
        v.pan(by: CGSize(width: -40, height: -30))
        c.canvas.setViewport(v)
        XCTAssertEqual(c.horizontalRuler.origin, before - 40)
        XCTAssertEqual(c.verticalRuler.origin, c.canvas.viewport.origin.y)
        // Inches at the document's DPI, then at 300 and 72 DPI.
        settings.units = .inches
        XCTAssertEqual(c.horizontalRuler.scale.unit, .inches)
        XCTAssertEqual(c.horizontalRuler.scale.dpi, wc.documentDPI)
        for dpi in [300.0, 72] {
            c.rulerDPI = dpi
            let s = c.horizontalRuler.scale
            XCTAssertEqual(s.pointsPerUnit, 8 * dpi, accuracy: 1e-9)
            for t in c.horizontalRuler.ticks {
                XCTAssertEqual(t.position, Double(c.canvas.viewport.origin.x) + t.value * 8 * dpi, accuracy: 1e-6)
            }
        }
        // Selection and pointer marks, in view points.
        c.rulerDPI = wc.documentDPI
        wc.selectAllPixels()
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        let vp = c.canvas.viewport
        XCTAssertEqual(c.horizontalRuler.highlight, vp.origin.x...(vp.origin.x + 512 * 8))
        c.rulerCursor = CGPoint(x: 10, y: 20)
        XCTAssertEqual(c.horizontalRuler.marker, vp.origin.x + 80)
        XCTAssertEqual(c.verticalRuler.marker, vp.origin.y + 160)
        settings.rulers = false
        XCTAssertFalse(c.rulersVisible)
        XCTAssertTrue(c.horizontalRuler.isHidden)
        XCTAssertEqual(c.canvas.frame.origin, .zero)
    }

    func testUnitsChangeTheStatusBarAndTheMenuChecks() throws {
        let wc = try fx.open()
        let dpi = wc.documentDPI
        let menu = try viewMenu()
        let status = wc.statusBar
        XCTAssertEqual(status.units.titleOfSelectedItem, "px")
        XCTAssertEqual(status.imageValue.stringValue, "512 × 384 px")
        for title in ["Pixels", "Inches", "Centimeters"] {
            let item = try XCTUnwrap(menu.item(withTitle: title))
            XCTAssertTrue(wc.validateMenuItem(item), title)
            XCTAssertEqual(item.state, title == "Pixels" ? .on : .off, title)
            XCTAssertTrue(status.units.item(withTitle: MeasurementUnit.allCases.first { $0.title == title }!.abbreviation)!.isEnabled)
        }
        wc.setUnits(menu.item(withTitle: "Inches"))
        XCTAssertEqual(settings.units, .inches)
        XCTAssertEqual(status.units.titleOfSelectedItem, "in")
        XCTAssertEqual(status.imageValue.stringValue,
                       String(format: "%.2f × %.2f in", 512 / dpi, 384 / dpi))
        let inches = try XCTUnwrap(menu.item(withTitle: "Inches"))
        _ = wc.validateMenuItem(inches)
        XCTAssertEqual(inches.state, .on)
        // The status bar popup changes the same setting.
        status.units.selectItem(withTitle: "cm")
        status.units.sendAction(status.units.action, to: status.units.target)
        XCTAssertEqual(settings.units, .centimeters)
        XCTAssertEqual(wc.container.rulerUnit, .centimeters)
        XCTAssertEqual(status.imageValue.stringValue,
                       String(format: "%.2f × %.2f cm", 512 / dpi * 2.54, 384 / dpi * 2.54))
        wc.selectAllPixels()
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        wc.updateStatus()
        XCTAssertEqual(status.selectionValue.stringValue,
                       String(format: "%.2f × %.2f cm", 512 / dpi * 2.54, 384 / dpi * 2.54))
    }

    func testViewMenuEnabledStatesAndZoomPresets() throws {
        let wc = try fx.open()
        let menu = try viewMenu()
        func item(_ t: String) throws -> NSMenuItem { try XCTUnwrap(menu.item(withTitle: t), t) }
        let presets = try XCTUnwrap(menu.item(withTitle: "Zoom Presets")?.submenu)
        XCTAssertEqual(presets.items.map(\.title), ZoomPresets.values.reversed().map(ZoomPresets.label))
        // Every preset sets that zoom and shows the check mark.
        for p in presets.items {
            XCTAssertTrue(wc.validateMenuItem(p), p.title)
            wc.zoomToPreset(p)
            XCTAssertEqual(wc.canvas.viewport.zoom, MenuSkeleton.presetZoom(forTitle: p.title)!, p.title)
            _ = wc.validateMenuItem(p)
            XCTAssertEqual(p.state, .on, p.title)
        }
        XCTAssertEqual(wc.canvas.viewport.zoom, 0.01, "the last preset is 1%")
        XCTAssertFalse(wc.validateMenuItem(try item("Zoom Out")), "at 1%")
        XCTAssertFalse(wc.canvas.validateMenuItem(try item("Zoom Out")))
        XCTAssertTrue(wc.validateMenuItem(try item("Zoom In")))
        wc.zoomToPreset(presets.items[0])
        XCTAssertEqual(wc.canvas.viewport.zoom, 32)
        XCTAssertFalse(wc.validateMenuItem(try item("Zoom In")), "at 3200%")
        XCTAssertTrue(wc.validateMenuItem(try item("Zoom Out")))
        _ = wc.validateMenuItem(presets.items[1])
        XCTAssertEqual(presets.items[1].state, .off, "only the current preset is checked")
        // Actual Size, Zoom to Window.
        wc.actualSize(try item("Actual Size"))
        XCTAssertEqual(wc.canvas.viewport.zoom, 1)
        wc.zoomToWindow(try item("Zoom to Window"))
        XCTAssertTrue(wc.canvas.isFitted)
        // Zoom to Selection needs a selection.
        let toSelection = try item("Zoom to Selection")
        XCTAssertFalse(wc.validateMenuItem(toSelection))
        wc.editor.run("selection.rectangle", ["x": 10, "y": 10, "width": 40, "height": 20])
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        XCTAssertTrue(wc.validateMenuItem(toSelection))
        wc.zoomToSelection(toSelection)
        let vs = wc.canvas.viewport.viewSize
        XCTAssertEqual(wc.canvas.viewport.zoom, ZoomPresets.clamp(min(vs.width / 42, vs.height / 22)), accuracy: 1e-9)
        // Rulers and Pixel Grid show their state.
        let rulers = try item("Rulers"), grid = try item("Pixel Grid")
        XCTAssertEqual(rulers.keyEquivalent, "r")
        XCTAssertEqual(rulers.keyEquivalentModifierMask, [.command, .option])
        XCTAssertTrue(wc.validateMenuItem(rulers))
        XCTAssertEqual(rulers.state, .off)
        wc.toggleRulers(rulers)
        _ = wc.validateMenuItem(rulers)
        XCTAssertEqual(rulers.state, .on)
        XCTAssertTrue(wc.validateMenuItem(grid))
        XCTAssertEqual(grid.state, .off)
        // The canvas handles Pixel Grid when it has focus; the setting follows.
        wc.canvas.togglePixelGrid(grid)
        XCTAssertTrue(settings.pixelGrid)
        _ = wc.validateMenuItem(grid)
        XCTAssertEqual(grid.state, .on)
        XCTAssertTrue((wc.commandButtons["grid"] as? ToggleButton)?.isOn == true)
        // Settings… works with no document window.
        let app = try XCTUnwrap(MenuSkeleton.build(effects: [], panelTarget: nil).item(withTitle: "OpenPainter")?.submenu)
        let settingsItem = try XCTUnwrap(app.item(withTitle: "Settings…"))
        XCTAssertTrue(settingsItem.target === SettingsMenuTarget.shared)
        XCTAssertEqual(settingsItem.keyEquivalent, ",")
    }

    func testChangesApplyToEveryOpenWindowAtOnce() throws {
        let a = try fx.open(name: "a.png")
        let b = try fx.open(name: "b.png")
        a.togglePixelGrid(nil)
        XCTAssertTrue(a.canvas.pixelGrid)
        XCTAssertTrue(b.canvas.pixelGrid, "the grid setting reaches the other window")
        XCTAssertTrue(try XCTUnwrap(b.canvas.engineView).info().state.pixelGrid)
        b.toggleRulers(nil)
        XCTAssertTrue(a.container.rulersVisible)
        XCTAssertTrue(b.container.rulersVisible)
        a.statusBar.units.selectItem(withTitle: "in")
        a.statusBar.units.sendAction(a.statusBar.units.action, to: a.statusBar.units.target)
        XCTAssertEqual(b.statusBar.units.titleOfSelectedItem, "in")
        XCTAssertEqual(b.container.rulerUnit, .inches)
    }

    func testSettingsWindowSavesAndANewWindowUsesTheSettings() throws {
        let wc = try fx.open()
        SettingsWindowController.show(settings: settings)
        let s = try XCTUnwrap(SettingsWindowController.shared)
        XCTAssertEqual(s.window?.title, "Settings")
        XCTAssertEqual(s.appearancePopup.titleOfSelectedItem, "System")
        XCTAssertEqual(s.unitsPopup.titleOfSelectedItem, "Pixels")
        XCTAssertEqual(s.rulersCheck.state, .off)
        XCTAssertEqual(s.gridCheck.state, .off)
        s.appearancePopup.selectItem(withTitle: "Dark")
        s.changed(s.appearancePopup)
        s.unitsPopup.selectItem(withTitle: "Centimeters")
        s.changed(s.unitsPopup)
        s.rulersCheck.state = .on
        s.changed(s.rulersCheck)
        s.gridCheck.state = .on
        s.changed(s.gridCheck)
        // The open window changed at once.
        XCTAssertEqual(NSApp.appearance?.name, .darkAqua)
        XCTAssertTrue(wc.container.rulersVisible)
        XCTAssertTrue(wc.canvas.pixelGrid)
        XCTAssertEqual(wc.statusBar.units.titleOfSelectedItem, "cm")
        // Stored in UserDefaults: a fresh AppSettings on the same defaults reads them.
        let again = AppSettings(defaults: fx.defaults)
        XCTAssertEqual(again.appearance, .dark)
        XCTAssertEqual(again.units, .centimeters)
        XCTAssertTrue(again.rulers)
        XCTAssertTrue(again.pixelGrid)
        XCTAssertEqual(fx.defaults.string(forKey: "OPSettings.units"), "centimeters")
        // A window opened afterwards starts with them.
        let second = try fx.open(name: "second.png")
        XCTAssertTrue(second.container.rulersVisible)
        XCTAssertTrue(second.canvas.pixelGrid)
        XCTAssertEqual(second.container.rulerUnit, .centimeters)
        XCTAssertEqual(second.statusBar.units.titleOfSelectedItem, "cm")
        // A change made elsewhere shows in the open Settings window.
        settings.units = .inches
        XCTAssertEqual(s.unitsPopup.titleOfSelectedItem, "Inches")
        s.appearancePopup.selectItem(withTitle: "System")
        s.changed(s.appearancePopup)
        XCTAssertNil(NSApp.appearance)
    }

    func testCanvasSurroundFollowsTheAppearance() throws {
        let wc = try fx.open()
        let canvas = wc.canvas
        let view = try XCTUnwrap(canvas.engineView)
        let light = Double(0x77) / 255, dark = Double(0x25) / 255
        XCTAssertEqual(MetalCanvasView.stageComponents(for: NSAppearance(named: .aqua)!), [light, light, light])
        XCTAssertEqual(MetalCanvasView.stageComponents(for: NSAppearance(named: .darkAqua)!), [dark, dark, dark])
        for (choice, want) in [(AppearanceChoice.dark, dark), (.light, light), (.dark, dark)] {
            settings.appearance = choice
            XCTAssertTrue(waitUntil { canvas.sentBackground == [want, want, want] }, "\(choice)")
            let got = try view.info().background
            for g in got { XCTAssertEqual(g, want, accuracy: 1e-6, "\(choice) reached the engine") }
        }
        XCTAssertEqual(try view.info().frameErrors, 0)
    }

    /// With OP_PANEL_PNG_DIR set, renders every panel, the Settings window, the
    /// options row, strip and status bar, and the rulers in light and dark.
    func testRenderEveryPanelLightAndDarkForVisualCheck() throws {
        guard let dir = ProcessInfo.processInfo.environment["OP_PANEL_PNG_DIR"] else { return }
        let wc = try fx.open()
        settings.rulers = true
        wc.editor.run("selection.rectangle", ["x": 60, "y": 40, "width": 200, "height": 120])
        XCTAssertTrue(waitUntil { wc.editor.selection != nil })
        wc.container.rulerCursor = CGPoint(x: 300, y: 200)
        SettingsWindowController.show(settings: settings)
        let s = try XCTUnwrap(SettingsWindowController.shared)
        func write(_ view: NSView, _ name: String) throws {
            view.needsLayout = true
            view.layoutSubtreeIfNeeded()
            for sub in view.subviewsDeep { sub.needsDisplay = true }
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            try XCTUnwrap(rep.representation(using: .png, properties: [:]))
                .write(to: URL(fileURLWithPath: dir).appendingPathComponent("m56-\(name).png"))
        }
        for (name, choice) in [("light", AppearanceChoice.light), ("dark", .dark)] {
            settings.appearance = choice
            for (kind, panel) in fx.workspace.panels.panels {
                try write(try XCTUnwrap(panel.contentView), "panel-\(kind)-\(name)")
            }
            try write(try XCTUnwrap(s.window?.contentView), "settings-\(name)")
            try write(wc.optionsBar, "options-\(name)")
            try write(wc.strip, "strip-\(name)")
            try write(wc.statusBar, "status-\(name)")
            for u in MeasurementUnit.allCases {
                settings.units = u
                let c = wc.container
                // The Metal canvas does not draw into a cached image, so the rulers go alone.
                try write(c.horizontalRuler, "ruler-h-\(u.abbreviation)-\(name)")
                try write(c.verticalRuler, "ruler-v-\(u.abbreviation)-\(name)")
            }
            settings.units = .pixels
            if let root = wc.window?.contentView { try write(root, "window-\(name)") }
        }
    }
}
