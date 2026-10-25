// M36: the Colors panel (swatches, hex, wheel, More/Less, palette and palette
// files) and the Color Picker tool (sample sizes, edge clipping, layer and
// image sampling, after-click), plus secondary-color painting.
import AppKit
@testable import OpenPainterKit
import XCTest

// MARK: Pure color math and palette files

@MainActor
final class ColorModelTests: XCTestCase {
    func testHexFieldValidation() {
        XCTAssertEqual(DocColor(hex: "#CF684D"), DocColor(r: 0xCF, g: 0x68, b: 0x4D))
        XCTAssertEqual(DocColor(hex: "cf684d"), DocColor(r: 0xCF, g: 0x68, b: 0x4D))
        XCTAssertEqual(DocColor(hex: " 00FF0080 "), DocColor(r: 0, g: 255, b: 0, a: 0x80))
        // Regression: UInt32(_:radix:) accepts a leading "+", so "+12345" used to pass.
        for bad in ["+12345", "+1234567", "-12345", "12345", "1234567", "GGGGGG", "#12 456", "", "#", "0x1234",
                    "１２３４５６"] {
            XCTAssertNil(DocColor(hex: bad), bad)
        }
        XCTAssertEqual(DocColor(r: 1, g: 2, b: 3).hex, "#010203")
        XCTAssertEqual(DocColor(r: 1, g: 2, b: 3, a: 4).hex, "#01020304")
        // Channel fields take whole numbers in range only.
        XCTAssertEqual(ChannelRow.parse(" 255 ", range: 0...255), 255)
        XCTAssertNil(ChannelRow.parse("256", range: 0...255))
        XCTAssertNil(ChannelRow.parse("-1", range: 0...255))
        XCTAssertNil(ChannelRow.parse("1.5", range: 0...255))
        XCTAssertNil(ChannelRow.parse("abc", range: 0...360))
        XCTAssertEqual(ChannelRow.parse("360", range: 0...360), 360)
    }

    func testHSVConversionRoundTripsEveryByteTriple() {
        XCTAssertEqual(HSV(DocColor(r: 255, g: 0, b: 0)), HSV(h: 0, s: 100, v: 100))
        XCTAssertEqual(HSV(DocColor(r: 0, g: 0, b: 255)), HSV(h: 240, s: 100, v: 100))
        XCTAssertEqual(HSV(h: 120, s: 100, v: 100).color(), DocColor(r: 0, g: 255, b: 0))
        XCTAssertEqual(HSV(h: 360, s: 100, v: 100).color(), DocColor(r: 255, g: 0, b: 0))
        XCTAssertEqual(HSV(h: 30, s: 0, v: 50).color(alpha: 9), DocColor(r: 128, g: 128, b: 128, a: 9))
        // Exact round trip with full-precision HSV for a grid of colors.
        for r in stride(from: 0, through: 255, by: 15) {
            for g in stride(from: 0, through: 255, by: 17) {
                for b in stride(from: 0, through: 255, by: 51) {
                    let c = DocColor(r: UInt8(r), g: UInt8(g), b: UInt8(b))
                    XCTAssertEqual(HSV(c).color(), c)
                }
            }
        }
    }

    func testWheelModifierMath() {
        let current = HSV(h: 90, s: 40, v: 80)
        // No keys: the angle is the hue, the distance the saturation.
        var p = ColorWheelMath.pick(dx: 0.5, dy: 0, current: current, mods: [])
        XCTAssertEqual(p.h, 0, accuracy: 1e-9)
        XCTAssertEqual(p.s, 50, accuracy: 1e-9)
        p = ColorWheelMath.pick(dx: 0, dy: 2, current: current, mods: [])
        XCTAssertEqual(p.h, 90, accuracy: 1e-9)
        XCTAssertEqual(p.s, 100, accuracy: 1e-9, "outside the rim clamps to 100")
        p = ColorWheelMath.pick(dx: -0.3, dy: -0.3, current: current, mods: [])
        XCTAssertEqual(p.h, 225, accuracy: 1e-9)
        // Command (paint.net Ctrl): same radius, only the hue changes.
        p = ColorWheelMath.pick(dx: -0.9, dy: 0, current: current, mods: .keepRadius)
        XCTAssertEqual(p.h, 180, accuracy: 1e-9)
        XCTAssertEqual(p.s, 40)
        // Option (Alt): same spoke, saturation is the projection onto it.
        p = ColorWheelMath.pick(dx: 0.3, dy: 0.6, current: current, mods: .keepSpoke)
        XCTAssertEqual(p.h, 90)
        XCTAssertEqual(p.s, 60, accuracy: 1e-9)
        p = ColorWheelMath.pick(dx: 0.3, dy: -0.6, current: current, mods: .keepSpoke)
        XCTAssertEqual(p.s, 0, "the far side of the center is the center")
        // Shift: the hue snaps to 15 degree spokes; saturation is free.
        p = ColorWheelMath.pick(dx: cos(37 * .pi / 180) * 0.7, dy: sin(37 * .pi / 180) * 0.7, current: current,
                                mods: .snapSpokes)
        XCTAssertEqual(p.h, 30, accuracy: 1e-9)
        XCTAssertEqual(p.s, 70, accuracy: 1e-9)
        p = ColorWheelMath.pick(dx: cos(-5 * .pi / 180), dy: sin(-5 * .pi / 180), current: current, mods: .snapSpokes)
        XCTAssertEqual(p.h, 0, accuracy: 1e-9, "355 degrees snaps to 0, not 360")
        // Command-Shift: 15 degree steps on the same radius.
        p = ColorWheelMath.pick(dx: cos(52 * .pi / 180) * 0.2, dy: sin(52 * .pi / 180) * 0.2, current: current,
                                mods: [.keepRadius, .snapSpokes])
        XCTAssertEqual(p.h, 45, accuracy: 1e-9)
        XCTAssertEqual(p.s, 40)
        // Mac keys map to the paint.net modifiers.
        XCTAssertEqual(ColorWheelView.modifiers([.command]), .keepRadius)
        XCTAssertEqual(ColorWheelView.modifiers([.option]), .keepSpoke)
        XCTAssertEqual(ColorWheelView.modifiers([.shift, .command]), [.snapSpokes, .keepRadius])
        XCTAssertEqual(ColorWheelView.modifiers([.control]), [])
        // The marker sits where a plain click on it would pick the same color.
        let m = ColorWheelMath.point(for: current)
        p = ColorWheelMath.pick(dx: m.x, dy: m.y, current: HSV(h: 0, s: 0, v: 0), mods: [])
        XCTAssertEqual(p.h, 90, accuracy: 1e-9)
        XCTAssertEqual(p.s, 40, accuracy: 1e-9)
    }

    func testPaletteParsingMalformedLinesSlotCountAndAlpha() {
        let text = """
            ; Paint.NET Palette File
            FF000000
            80FF0000 ; half-transparent red, with a trailing comment
              ff00ff00\t
            00FFFFFF

            FF12345
            GG000000
            FF0000FF00
            #FF0000FF
            ;FFFFFFFF commented out
            FFABCDEF\r
            """
        let r = Palette.parse(text)
        XCTAssertEqual(r.colorCount, 5)
        XCTAssertEqual(r.palette.colors.count, 96)
        XCTAssertEqual(r.palette.colors[0], DocColor(r: 0, g: 0, b: 0, a: 255))
        XCTAssertEqual(r.palette.colors[1], DocColor(r: 255, g: 0, b: 0, a: 0x80), "aa is alpha")
        XCTAssertEqual(r.palette.colors[2], DocColor(r: 0, g: 255, b: 0, a: 255), "lower case and spaces")
        XCTAssertEqual(r.palette.colors[3], DocColor(r: 255, g: 255, b: 255, a: 0), "alpha 00 stays")
        XCTAssertEqual(r.palette.colors[4], DocColor(r: 0xAB, g: 0xCD, b: 0xEF), "CRLF line ends")
        XCTAssertEqual(r.palette.colors[5], Palette.padding, "missing slots are white")
        XCTAssertEqual(r.palette.colors[95], DocColor(r: 255, g: 255, b: 255, a: 255))
        XCTAssertEqual(r.issues.map(\.line), [7, 8, 9, 10])
        XCTAssertTrue(r.issues.allSatisfy { $0.reason.contains("aarrggbb") })
        XCTAssertTrue(r.summary.contains("Loaded 5 of 96"), r.summary)
        XCTAssertTrue(r.summary.contains("skipped 4 malformed lines (7, 8, 9, 10)"), r.summary)

        // More than 96 colors: the rest are ignored and reported.
        let many = (0..<100).map { String(format: "FF%06X", $0) }.joined(separator: "\n")
        let m = Palette.parse(many)
        XCTAssertEqual(m.colorCount, 96)
        XCTAssertEqual(m.palette.colors[95], DocColor(r: 0, g: 0, b: 95))
        XCTAssertEqual(m.issues.count, 4)
        XCTAssertEqual(m.issues.first?.line, 97)
        XCTAssertTrue(m.summary.contains("ignored 4 colors after slot 96 (from line 97)"), m.summary)
        // Nothing usable.
        XCTAssertEqual(Palette.parse("; only a comment\n\nnope").colorCount, 0)
    }

    func testDefaultPaletteAndFileText() {
        let p = Palette.standard
        XCTAssertEqual(p.colors.count, 96)
        XCTAssertEqual(p.colors[0], DocColor(r: 0, g: 0, b: 0))
        XCTAssertEqual(p.colors[7], DocColor(r: 255, g: 255, b: 255))
        XCTAssertEqual(p.colors[8], DocColor(r: 255, g: 0, b: 0))
        XCTAssertEqual(Set(p.colors.map(\.hex)).count, 96, "no repeated colors")
        XCTAssertTrue(p.colors.allSatisfy { $0.a == 255 })
        // The file text parses back to the same palette with no issues.
        let text = p.fileText
        XCTAssertTrue(text.hasPrefix(";"))
        XCTAssertTrue(text.contains("\nFFFF0000\n"), "AARRGGBB, red")
        let back = Palette.parse(text)
        XCTAssertEqual(back.palette, p)
        XCTAssertEqual(back.issues, [])
        XCTAssertEqual(back.colorCount, 96)
    }

    func testMoreLessFramesGiveToolsA12PointGap() {
        // A 1280 x 800 window's stage at the bottom-left of a screen (y up).
        let stage = CGRect(x: 0, y: 0, width: 1280, height: 574)
        let d = PanelLayout.defaults(stage: stage)
        let r = PanelLayout.colorsFrames(colors: d[.colors]!, tools: d[.tools]!, expanded: true, bounds: stage)
        XCTAssertEqual(r.colors.minY, d[.colors]!.minY, "the bottom edge stays")
        XCTAssertEqual(r.tools!.height, 94, "Tools shortens to its 94 pt minimum")
        XCTAssertEqual(r.tools!.maxY, d[.tools]!.maxY, "Tools keeps its top")
        XCTAssertEqual(r.tools!.minY - r.colors.maxY, 12, "12 pt gap")
        XCTAssertLessThanOrEqual(r.colors.height, 470)
        XCTAssertGreaterThan(r.colors.height, 246)
        // A tall stage: Colors reaches 470 pt and Tools only shortens as needed.
        let tall = CGRect(x: 0, y: 0, width: 1280, height: 1000)
        let t = PanelLayout.defaults(stage: tall)
        let r2 = PanelLayout.colorsFrames(colors: t[.colors]!, tools: t[.tools]!, expanded: true, bounds: tall)
        XCTAssertEqual(r2.colors.height, 470)
        XCTAssertGreaterThanOrEqual(r2.tools!.minY - r2.colors.maxY, 12)
        // Less goes back to 246 pt and restores Tools.
        let back = PanelLayout.colorsFrames(colors: r.colors, tools: r.tools, expanded: false, bounds: stage,
                                            restoreTools: d[.tools])
        XCTAssertEqual(back.colors, d[.colors])
        XCTAssertEqual(back.tools, d[.tools])
    }
}

// MARK: The panel, the picker and painting with the secondary color

@MainActor
final class ColorsPanelTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    var tools: ToolState { fx.workspace.tools }
    var colors: ColorsPanelView { fx.workspace.colorsView }

    func testSwatchesSwapResetAndActiveSlot() throws {
        let view = colors
        let red = DocColor(r: 255, g: 0, b: 0), blue = DocColor(r: 0, g: 0, b: 255)
        tools.primary = red
        tools.secondary = blue
        view.swapButton.performClick(nil)
        XCTAssertEqual(tools.primary, blue)
        XCTAssertEqual(tools.secondary, red)
        tools.swapColors()
        XCTAssertEqual(tools.primary, red)
        view.resetButton.performClick(nil)
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 0, b: 0))
        XCTAssertEqual(tools.secondary, DocColor(r: 255, g: 255, b: 255))
        // Clicking the other swatch moves the notch; edits then go to that slot.
        XCTAssertEqual(tools.activeSlot, .primary)
        XCTAssertEqual(view.swatches.slot(at: NSPoint(x: 50, y: 50)), .secondary)
        XCTAssertEqual(view.swatches.slot(at: NSPoint(x: 5, y: 5)), .primary)
        XCTAssertEqual(view.swatches.slot(at: NSPoint(x: 50, y: 2)), nil)
        XCTAssertTrue(view.swatches.accessibilityPerformPress())
        XCTAssertEqual(tools.activeSlot, .secondary)
        view.hexField.stringValue = "123456"
        view.hexField.sendAction(view.hexField.action, to: view.hexField.target)
        XCTAssertEqual(tools.secondary, DocColor(r: 0x12, g: 0x34, b: 0x56))
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 0, b: 0))
        XCTAssertEqual(view.hexField.accessibilityLabel(), "Secondary color hex")
        tools.toggleActiveSlot()
        XCTAssertEqual(tools.activeSlot, .primary)
        // X and C on the canvas.
        let wc = try fx.open()
        wc.window?.makeFirstResponder(wc.canvas)
        let (p, s) = (tools.primary, tools.secondary)
        wc.canvas.keyDown(with: keyEvent("x", keyCode: 7))
        XCTAssertEqual(tools.primary, s)
        XCTAssertEqual(tools.secondary, p)
        wc.canvas.keyDown(with: keyEvent("c", keyCode: 8))
        XCTAssertEqual(tools.activeSlot, .secondary)
        XCTAssertEqual(tools.tool, .paintbrush, "X and C pick no tool")
    }

    func testHexFieldRejectsBadInputAndKeepsAlpha() {
        let view = colors
        tools.primary = DocColor(r: 0xCF, g: 0x68, b: 0x4D)
        for bad in ["zzz", "+12345", "12345", "#1234567"] {
            view.hexField.stringValue = bad
            view.hexField.sendAction(view.hexField.action, to: view.hexField.target)
            XCTAssertEqual(tools.primary, DocColor(r: 0xCF, g: 0x68, b: 0x4D), bad)
            XCTAssertEqual(view.hexField.stringValue, "#CF684D", "the field shows the color again")
        }
        view.hexField.stringValue = "#00FF0080"
        view.hexField.sendAction(view.hexField.action, to: view.hexField.target)
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 255, b: 0, a: 0x80))
        XCTAssertEqual(view.hexField.stringValue, "#00FF0080")
    }

    func testEndingAnUnchangedFieldEditKeepsAColorSetElsewhere() throws {
        // Regression: the hex field (or a channel field) was editing, the color
        // changed elsewhere, and ending the edit wrote the stale text back.
        _ = try fx.open()
        let view = colors
        let panel = fx.workspace.panels.panels[.colors]!
        view.setExpanded(true)
        tools.primary = DocColor(r: 0xCF, g: 0x68, b: 0x4D)
        XCTAssertTrue(panel.makeFirstResponder(view.hexField))
        XCTAssertNotNil(view.hexField.currentEditor())
        tools.primary = DocColor(r: 1, g: 2, b: 3)
        view.hexField.sendAction(view.hexField.action, to: view.hexField.target)
        XCTAssertEqual(tools.primary, DocColor(r: 1, g: 2, b: 3))
        XCTAssertEqual(view.hexField.stringValue, "#010203")
        let red = view.channels[.red]!.field
        XCTAssertTrue(panel.makeFirstResponder(red))
        tools.primary = DocColor(r: 9, g: 2, b: 3)
        red.sendAction(red.action, to: red.target)
        XCTAssertEqual(tools.primary, DocColor(r: 9, g: 2, b: 3))
        XCTAssertEqual(red.stringValue, "9")
        view.setExpanded(false)
    }

    func testWheelEditsTheActiveSlotAndRightClickTheOther() {
        let view = colors
        tools.primary = DocColor(r: 255, g: 0, b: 0)
        tools.secondary = DocColor(r: 0, g: 0, b: 0)
        // Plain click at the top of the wheel: hue 90, full saturation, value kept.
        view.wheelPicked(dx: 0, dy: 1, mods: [], other: false)
        XCTAssertEqual(tools.primary, HSV(h: 90, s: 100, v: 100).color())
        XCTAssertEqual(view.wheel.hsv.h, 90, accuracy: 1e-9)
        // Command keeps the radius: half way in, the saturation stays 100.
        view.wheelPicked(dx: -0.5, dy: 0, mods: .keepRadius, other: false)
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 255, b: 255))
        // Option keeps the spoke (180): only saturation changes.
        view.wheelPicked(dx: -0.5, dy: 0.4, mods: .keepSpoke, other: false)
        XCTAssertEqual(view.hsv.h, 180)
        XCTAssertEqual(view.hsv.s, 50, accuracy: 1e-9)
        XCTAssertEqual(tools.primary, HSV(h: 180, s: 50, v: 100).color())
        // Right-click goes to the secondary slot. It was black, so value becomes 100.
        view.wheelPicked(dx: 1, dy: 0, mods: [], other: true)
        XCTAssertEqual(tools.secondary, DocColor(r: 255, g: 0, b: 0))
        XCTAssertEqual(tools.primary, HSV(h: 180, s: 50, v: 100).color(), "primary unchanged")
        // Alpha survives a wheel pick.
        tools.primary = DocColor(r: 10, g: 20, b: 30, a: 77)
        view.wheelPicked(dx: 0, dy: -1, mods: [], other: false)
        XCTAssertEqual(tools.primary.a, 77)
    }

    func testMoreShowsSlidersFieldsAndAll96AndResizesThePanels() throws {
        let wc = try fx.open()
        _ = wc
        let view = colors
        let panels = fx.workspace.panels
        let colorsBefore = panels.frame(.colors), toolsBefore = panels.frame(.tools)
        XCTAssertEqual(view.palette.cells.count, 32, "compact shows the first 32")
        XCTAssertTrue(view.channels.values.allSatisfy(\.isHidden))
        XCTAssertEqual(view.moreButton.title, "More »")
        view.moreButton.performClick(nil)
        XCTAssertTrue(view.isExpanded)
        XCTAssertEqual(view.moreButton.title, "« Less")
        XCTAssertEqual(view.palette.cells.count, 96)
        XCTAssertTrue(view.channels.values.allSatisfy { !$0.isHidden })
        let c = panels.frame(.colors), t = panels.frame(.tools)
        XCTAssertGreaterThan(c.height, colorsBefore.height)
        XCTAssertLessThanOrEqual(c.height, 470)
        XCTAssertEqual(c.minY, colorsBefore.minY)
        XCTAssertGreaterThanOrEqual(t.minY - c.maxY, 12 - 0.5, "Tools stays 12 pt above Colors")
        XCTAssertGreaterThanOrEqual(t.height, 94)
        XCTAssertTrue(PanelLayout.noOverlap(panels.shownFrames, gap: 11))
        // The palette scrolls inside the expanded panel.
        view.window?.layoutIfNeeded()
        view.layoutSubtreeIfNeeded()
        XCTAssertLessThan(view.palette.frame.height, PaletteView.height(rows: 12))
        // Fields and sliders edit the active color.
        tools.primary = DocColor(r: 0, g: 0, b: 0)
        let red = view.channels[.red]!
        red.field.stringValue = "200"
        red.field.sendAction(red.field.action, to: red.field.target)
        XCTAssertEqual(tools.primary, DocColor(r: 200, g: 0, b: 0))
        red.field.stringValue = "300"
        red.field.sendAction(red.field.action, to: red.field.target)
        XCTAssertEqual(tools.primary, DocColor(r: 200, g: 0, b: 0), "out of range is refused")
        XCTAssertEqual(red.field.stringValue, "200")
        view.channelChanged(.alpha, 128)
        XCTAssertEqual(tools.primary, DocColor(r: 200, g: 0, b: 0, a: 128))
        view.channelChanged(.hue, 120)
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 200, b: 0, a: 128))
        // Value 0 and back keeps hue and saturation.
        view.channelChanged(.value, 0)
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 0, b: 0, a: 128))
        view.channelChanged(.value, 100)
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 255, b: 0, a: 128))
        XCTAssertEqual(view.channels[.saturation]!.field.stringValue, "100")
        XCTAssertTrue(view.channels[.green]!.slider.accessibilityPerformDecrement())
        XCTAssertEqual(tools.primary.g, 254)
        let slider = view.channels[.blue]!.slider
        XCTAssertEqual(slider.value(at: slider.track.maxX + 10), 255)
        XCTAssertEqual(slider.value(at: slider.track.minX - 10), 0)
        // Less restores both panels.
        view.moreButton.performClick(nil)
        XCTAssertFalse(view.isExpanded)
        XCTAssertEqual(view.palette.cells.count, 32)
        XCTAssertEqual(panels.frame(.colors), colorsBefore)
        XCTAssertEqual(panels.frame(.tools), toolsBefore)
    }

    func testPaletteClicksAddModeAndChosenMark() {
        let view = colors
        tools.primary = DocColor(r: 1, g: 1, b: 1)
        tools.secondary = DocColor(r: 2, g: 2, b: 2)
        let p = tools.palette.colors
        view.palette.cells[8].mouseDown(with: mouse(.leftMouseDown))
        XCTAssertEqual(tools.primary, p[8])
        XCTAssertEqual(view.palette.chosen, 8, "the active color's swatch is marked")
        view.palette.cells[9].rightMouseDown(with: mouse(.rightMouseDown))
        XCTAssertEqual(tools.secondary, p[9])
        XCTAssertEqual(tools.primary, p[8])
        view.palette.cells[10].mouseDown(with: mouse(.leftMouseDown, [.shift]))
        XCTAssertEqual(tools.secondary, p[10], "Shift-click sets the other slot")
        view.palette.cells[11].mouseDown(with: mouse(.leftMouseDown, [.control]))
        XCTAssertEqual(tools.secondary, p[11], "Control-click is a right-click")
        // With the secondary slot active, a click goes there.
        tools.activeSlot = .secondary
        view.palette.cells[12].mouseDown(with: mouse(.leftMouseDown))
        XCTAssertEqual(tools.secondary, p[12])
        XCTAssertEqual(tools.primary, p[8])
        tools.activeSlot = .primary
        // Add mode puts the active color into the clicked slot, once.
        tools.primary = DocColor(r: 9, g: 8, b: 7)
        XCTAssertNil(view.palette.chosen)
        view.addColorButton.performClick(nil)
        XCTAssertTrue(view.palette.addMode)
        view.palette.cells[3].mouseDown(with: mouse(.leftMouseDown))
        XCTAssertEqual(tools.palette.colors[3], DocColor(r: 9, g: 8, b: 7))
        XCTAssertEqual(tools.primary, DocColor(r: 9, g: 8, b: 7))
        XCTAssertFalse(view.addColorButton.isOn)
        XCTAssertEqual(view.palette.chosen, 3)
        XCTAssertEqual(view.palette.cells.first?.frame.size, NSSize(width: 24, height: 24))
        view.palette.layoutSubtreeIfNeeded()
        let c0 = view.palette.cells[0].frame, c1 = view.palette.cells[1].frame, c8 = view.palette.cells[8].frame
        XCTAssertEqual(c1.minX - c0.maxX, 2, "2 pt gaps")
        XCTAssertEqual(c8.minY - c0.maxY, 2)
        XCTAssertEqual(c8.minX, c0.minX, "8 columns")
    }

    func testPaletteSaveAndLoadRoundTripAndReports() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("op-m36-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let view = colors
        var reports: [String] = []
        let old = view.onReport
        view.onReport = { reports.append($0); old?($0) }
        var p = tools.palette
        p.colors[0] = DocColor(r: 1, g: 2, b: 3, a: 4)
        p.colors[95] = DocColor(r: 250, g: 251, b: 252, a: 0)
        tools.palette = p
        let url = dir.appendingPathComponent("mine.txt")
        XCTAssertTrue(view.savePalette(to: url))
        let text = try String(contentsOf: url, encoding: .utf8)
        XCTAssertEqual(text.split(separator: "\n").filter { !$0.hasPrefix(";") && !$0.isEmpty }.count, 96)
        XCTAssertTrue(text.contains("\n04010203\n"))
        tools.palette = .standard
        let r = try XCTUnwrap(view.loadPalette(from: url))
        XCTAssertEqual(r.issues, [])
        XCTAssertEqual(tools.palette, p, "the round trip keeps every byte, alpha included")
        // A file with bad lines loads the good ones and reports the rest.
        let bad = dir.appendingPathComponent("bad.txt")
        try "FFFF0000\nnot a color\nFF00FF00\n".write(to: bad, atomically: true, encoding: .utf8)
        view.loadPalette(from: bad)
        XCTAssertEqual(tools.palette.colors[0], DocColor(r: 255, g: 0, b: 0))
        XCTAssertEqual(tools.palette.colors[1], DocColor(r: 0, g: 255, b: 0))
        XCTAssertEqual(tools.palette.colors[2], Palette.padding)
        XCTAssertEqual(view.lastReport, "bad.txt: Loaded 2 of 96 colors; skipped 1 malformed line (2).")
        // A file with no colors leaves the palette alone; a missing file is reported.
        let before = tools.palette
        let empty = dir.appendingPathComponent("empty.txt")
        try "; nothing\n".write(to: empty, atomically: true, encoding: .utf8)
        view.loadPalette(from: empty)
        XCTAssertEqual(tools.palette, before)
        XCTAssertTrue(view.lastReport?.contains("has no palette colors") == true)
        XCTAssertNil(view.loadPalette(from: dir.appendingPathComponent("missing.txt")))
        XCTAssertTrue(view.lastReport?.hasPrefix("Could not open missing.txt") == true)
        XCTAssertEqual(reports.count, 5)
    }

    // MARK: Color Picker

    /// The alpha-weighted average of the clipped window, from the composite.
    func expected(_ px: [UInt8], w: Int, h: Int, x: Int, y: Int, size: Int) -> DocColor {
        let half = size / 2
        var sum = [0, 0, 0], sa = 0, n = 0
        for py in max(y - half, 0)..<min(y + half + 1, h) {
            for qx in max(x - half, 0)..<min(x + half + 1, w) {
                let i = (py * w + qx) * 4
                let a = Int(px[i + 3])
                for c in 0..<3 { sum[c] += Int(px[i + c]) * a }
                sa += a
                n += 1
            }
        }
        guard sa > 0 else { return DocColor(r: 0, g: 0, b: 0, a: 0) }
        func div(_ a: Int, _ b: Int) -> UInt8 { UInt8((2 * a + b) / (2 * b)) }
        return DocColor(r: div(sum[0], sa), g: div(sum[1], sa), b: div(sum[2], sa), a: div(sa, n))
    }

    func testPickerSampleWindowIsClippedAtEveryEdgeAndCorner() throws {
        let wc = try fx.open()
        let px = try wc.editor.document.pixels()
        let (w, h) = (512, 384)
        let points = [(0, 190), (w - 1, 190), (250, 0), (250, h - 1), (0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1),
                      (3, 4), (100, 200)]
        for size in PickerSampleSize.allCases {
            for (x, y) in points {
                let got = wc.sampleColor(x: x, y: y, size: size, sampling: .image)
                XCTAssertEqual(got, expected(px, w: w, h: h, x: x, y: y, size: size.rawValue),
                               "(\(x), \(y)) \(size.label)")
            }
        }
        XCTAssertEqual(PickerSampleSize.allCases.map(\.rawValue), [1, 3, 5, 7, 9, 11, 31, 51])
    }

    func testPickerClicksLayerAndImageSamplingAndAfterClick() async throws {
        let wc = try fx.open("checker_600x300.png")
        let ed = wc.editor
        let px = try ed.document.pixels()
        let w = ed.info.width
        tools.tool = .colorPicker
        XCTAssertEqual(wc.canvas.tool, .click)
        XCTAssertNotNil(wc.optionsBar.pickerSize.superview, "picker options shown")
        XCTAssertNil(wc.optionsBar.toleranceField.superview)
        XCTAssertEqual(wc.optionsBar.pickerSize.numberOfItems, 8)
        XCTAssertEqual(wc.optionsBar.pickerAfterClick.itemTitles, PickerAfterClick.allCases.map(\.label))
        // Click: primary. Right-click: secondary.
        let at = { (x: Int, y: Int) -> DocColor in
            let i = (y * w + x) * 4
            return DocColor(r: px[i], g: px[i + 1], b: px[i + 2], a: px[i + 3])
        }
        wc.click(CGPoint(x: 5.5, y: 5.5))
        XCTAssertEqual(tools.primary, at(5, 5))
        wc.click(CGPoint(x: 300.2, y: 100.9), secondary: true)
        XCTAssertEqual(tools.secondary, at(300, 100))
        // Outside the canvas: nothing.
        let before = tools.primary
        wc.click(CGPoint(x: -1, y: 5))
        wc.click(CGPoint(x: 600, y: 5))
        XCTAssertEqual(tools.primary, before)
        // A new top layer filled with half-transparent blue: Layer sampling reads
        // that layer, Image sampling the composite. Command-click is Image.
        let added = await ed.runAndWait("layer.add", ["name": "Top"])
        XCTAssertEqual(added, "ok")
        let activated = await ed.runAndWait("layer.set_active", ["index": 1])
        XCTAssertEqual(activated, "ok")
        let filled = await ed.runAndWait("tool.paint_bucket", ["x": 0, "y": 0, "tolerance": 0.0, "contiguous": false,
                                              "sampling": "layer", "antialias": false, "color": "0000ff80",
                                              "blend": "normal"])
        XCTAssertEqual(filled, "ok")
        let composite = try ed.document.pixels()
        let i = (100 * w + 300) * 4
        let image = DocColor(r: composite[i], g: composite[i + 1], b: composite[i + 2], a: composite[i + 3])
        tools.settings.pickerSampling = .layer
        wc.click(CGPoint(x: 300, y: 100))
        XCTAssertEqual(tools.primary, DocColor(r: 0, g: 0, b: 255, a: 128))
        wc.click(CGPoint(x: 300, y: 100), modifiers: [.command])
        XCTAssertEqual(tools.primary, image)
        XCTAssertNotEqual(image, DocColor(r: 0, g: 0, b: 255, a: 128))
        tools.settings.pickerSampling = .image
        tools.primary = before
        wc.click(CGPoint(x: 300, y: 100))
        XCTAssertEqual(tools.primary, image)
        // Options bar controls set the settings.
        wc.optionsBar.pickerSize.selectItem(at: 2)
        wc.optionsBar.pickerSize.sendAction(wc.optionsBar.pickerSize.action, to: wc.optionsBar.pickerSize.target)
        XCTAssertEqual(tools.settings.pickerSize, .five)
        // After click: previous tool, then Pencil, then stay.
        tools.tool = .paintBucket
        tools.tool = .colorPicker
        tools.settings.pickerAfterClick = .previousTool
        wc.click(CGPoint(x: 10, y: 10))
        XCTAssertEqual(tools.tool, .paintBucket)
        tools.tool = .colorPicker
        tools.settings.pickerAfterClick = .pencil
        wc.click(CGPoint(x: 10, y: 10))
        XCTAssertEqual(tools.tool, .pencil)
        tools.tool = .colorPicker
        tools.settings.pickerAfterClick = .doNotSwitch
        wc.click(CGPoint(x: 10, y: 10))
        XCTAssertEqual(tools.tool, .colorPicker)
        XCTAssertEqual(PaintTool.colorPicker.isFunctional, true)
    }

    func testSecondaryColorFeedsBrushAndFill() throws {
        let wc = try fx.open()
        let ed = wc.editor
        tools.primary = DocColor(r: 255, g: 0, b: 0)
        tools.secondary = DocColor(r: 0, g: 0, b: 255)
        tools.tool = .pencil
        wc.stroke([CGPoint(x: 2.5, y: 2.5), CGPoint(x: 20.5, y: 2.5)], secondary: true)
        XCTAssertTrue(waitUntil { ed.info.historyLabels.last == "Pencil" }, ed.message ?? "")
        var px = try ed.document.pixels()
        XCTAssertEqual(Array(px[(2 * 512 + 10) * 4..<(2 * 512 + 10) * 4 + 4]), [0, 0, 255, 255])
        XCTAssertEqual(tools.strokeParams(tool: .paintbrush)["color"] as? String, "ff0000ff")
        XCTAssertEqual(tools.strokeParams(tool: .paintbrush, secondary: true)["color"] as? String, "0000ffff")
        XCTAssertEqual(tools.strokeParams(tool: .eraser, secondary: true)["color"] as? String, "000000ff")
        // Paint Bucket right-click fills with the secondary color.
        tools.tool = .paintBucket
        tools.settings.floodMode = .global
        tools.settings.tolerance = 100
        let n = ed.info.historyLabels.count
        wc.click(CGPoint(x: 300, y: 300), secondary: true)
        XCTAssertTrue(waitUntil { ed.info.historyLabels.count == n + 1 }, ed.message ?? "")
        px = try ed.document.pixels()
        XCTAssertEqual(Array(px[(300 * 512 + 300) * 4..<(300 * 512 + 300) * 4 + 3]), [0, 0, 255])
        XCTAssertEqual(tools.floodParams(tool: .paintBucket, x: 0, y: 0, secondary: true)["color"] as? String,
                       "0000ffff")
    }

    // MARK: Exact colors in both appearances

    /// Renders `view` into an sRGB bitmap at 2x under `appearance`.
    func render(_ view: NSView, _ appearance: NSAppearance.Name) -> NSBitmapImageRep {
        view.appearance = NSAppearance(named: appearance)
        let size = view.bounds.size
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2),
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: 0, bitsPerPixel: 32)!.retagging(with: .sRGB)!
        rep.size = size
        NSAppearance(named: appearance)!.performAsCurrentDrawingAppearance {
            view.layoutSubtreeIfNeeded()
            view.cacheDisplay(in: view.bounds, to: rep)
        }
        return rep
    }

    func bytes(_ rep: NSBitmapImageRep, _ x: Int, _ y: Int) -> [UInt8] {
        var p = [Int](repeating: 0, count: 4)
        rep.getPixel(&p, atX: x, y: y)
        return p.map { UInt8($0) }
    }

    func testDocumentColorsAreExactInLightAndDark() throws {
        let view = colors
        tools.primary = DocColor(r: 0xCF, g: 0x68, b: 0x4D)
        tools.secondary = DocColor(r: 0xE9, g: 0xE7, b: 0xDC)
        view.setExpanded(true)
        view.frame = NSRect(x: 0, y: 0, width: 244, height: 446)
        let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"]
        for name in [NSAppearance.Name.aqua, .darkAqua] {
            let rep = render(view, name)
            // Primary swatch center (10 + 16, 10 + 16) and secondary (10 + 46, 10 + 46), at 2x.
            XCTAssertEqual(bytes(rep, 2 * 20, 2 * 22), [0xCF, 0x68, 0x4D, 255], "\(name.rawValue) primary")
            XCTAssertEqual(bytes(rep, 2 * 52, 2 * 54), [0xE9, 0xE7, 0xDC, 255], "\(name.rawValue) secondary")
            // Palette swatches, in the palette view's own render.
            let pal = render(view.palette, name)
            let cell = view.palette.cells[10]
            let center = cell.convert(NSPoint(x: 12, y: 12), to: view.palette)
            XCTAssertEqual(bytes(pal, Int(center.x * 2), Int(center.y * 2)),
                           [tools.palette.colors[10].r, tools.palette.colors[10].g, tools.palette.colors[10].b, 255],
                           "\(name.rawValue) palette")
            if let out {
                let file = URL(fileURLWithPath: out).appendingPathComponent("colors-more-\(name == .aqua ? "light" : "dark").png")
                try rep.representation(using: .png, properties: [:])!.write(to: file)
            }
        }
        view.setExpanded(false)
        if let out {
            view.frame = NSRect(x: 0, y: 0, width: 244, height: ColorsPanelView.compactContentHeight)
            for name in [NSAppearance.Name.aqua, .darkAqua] {
                let rep = render(view, name)
                let file = URL(fileURLWithPath: out).appendingPathComponent("colors-compact-\(name == .aqua ? "light" : "dark").png")
                try rep.representation(using: .png, properties: [:])!.write(to: file)
            }
        }
        view.appearance = nil
    }

    func testCompactBodyFitsTheDefaultPanel() throws {
        _ = try fx.open()
        let panel = fx.workspace.panels.panels[.colors]!
        let content = panel.contentRect(forFrameRect: panel.frame)
        XCTAssertGreaterThanOrEqual(content.height, ColorsPanelView.compactContentHeight,
                                    "the compact Colors body fits the 246 pt panel")
        XCTAssertEqual(panel.frame.size, CGSize(width: 244, height: 246))
    }

    // MARK: Helpers

    func mouse(_ type: NSEvent.EventType, _ flags: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.mouseEvent(with: type, location: .zero, modifierFlags: flags, timestamp: 0, windowNumber: 0,
                           context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
    }
}
