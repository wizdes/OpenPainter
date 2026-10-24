import OpenPainterKit
import XCTest

final class ZoomPresetTests: XCTestCase {
    let presets: [Double] = [
        0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.12, 0.16, 0.25, 0.33, 0.50, 0.66, 1,
        2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32,
    ]

    func testPresetListIsPaintNets() {
        XCTAssertEqual(ZoomPresets.values, presets)
        XCTAssertEqual(ZoomPresets.minimum, 0.01)
        XCTAssertEqual(ZoomPresets.maximum, 32)
    }

    func testZoomInWalksEveryPresetFromOnePercentToThirtyTwoHundred() {
        var z = 0.01
        var seen = [z]
        for _ in 0..<40 {
            let next = ZoomPresets.nextLarger(z)
            if next == z { break }
            z = next
            seen.append(z)
        }
        XCTAssertEqual(seen, presets)
        XCTAssertEqual(ZoomPresets.nextLarger(32), 32, "upper bound")
    }

    func testZoomOutWalksEveryPresetBack() {
        var z = 32.0
        var seen = [z]
        for _ in 0..<40 {
            let next = ZoomPresets.nextSmaller(z)
            if next == z { break }
            z = next
            seen.append(z)
        }
        XCTAssertEqual(seen, presets.reversed())
        XCTAssertEqual(ZoomPresets.nextSmaller(0.01), 0.01, "lower bound")
    }

    func testStepsFromValuesBetweenPresetsFollowTheHalfPercentRule() {
        // GetNextLarger looks for the first preset >= zoom + 0.005.
        XCTAssertEqual(ZoomPresets.nextLarger(0.7), 1)
        XCTAssertEqual(ZoomPresets.nextLarger(0.996), 2)
        XCTAssertEqual(ZoomPresets.nextLarger(0.1475), 0.16)
        XCTAssertEqual(ZoomPresets.nextLarger(40), 32)
        XCTAssertEqual(ZoomPresets.nextLarger(0.001), 0.01)
        // GetNextSmaller: the preset before the first one >= zoom - 0.005.
        XCTAssertEqual(ZoomPresets.nextSmaller(0.7), 0.66)
        XCTAssertEqual(ZoomPresets.nextSmaller(1.004), 0.66)
        XCTAssertEqual(ZoomPresets.nextSmaller(1.006), 1)
        XCTAssertEqual(ZoomPresets.nextSmaller(0.1475), 0.12)
        XCTAssertEqual(ZoomPresets.nextSmaller(32), 24)
        // Like paint.net, a ratio above every preset steps "down" to the first one;
        // it cannot happen because zoom is clamped to 32.
        XCTAssertEqual(ZoomPresets.nextSmaller(100), 0.01)
        XCTAssertEqual(ZoomPresets.clamp(0), 0.01)
        XCTAssertEqual(ZoomPresets.clamp(99), 32)
    }

    func testFitIsTheSmallerAxisLessTenPointsAndNeverAboveActualSize() {
        let fit = ZoomPresets.fit(document: CGSize(width: 6000, height: 4000), view: CGSize(width: 960, height: 600))
        XCTAssertEqual(fit, min(950.0 / 6000, 590.0 / 4000), accuracy: 1e-12)
        XCTAssertEqual(ZoomPresets.fit(document: CGSize(width: 100, height: 50), view: CGSize(width: 960, height: 600)), 1)
        XCTAssertEqual(ZoomPresets.fit(document: CGSize(width: 1_000_000, height: 10), view: CGSize(width: 960, height: 600)), 0.01)
        XCTAssertEqual(ZoomPresets.fit(document: CGSize(width: 1000, height: 3000), view: CGSize(width: 1010, height: 610)), 0.2)
    }

    func testLabels() {
        XCTAssertEqual(ZoomPresets.label(1), "100%")
        XCTAssertEqual(ZoomPresets.label(0.33), "33%")
        XCTAssertEqual(ZoomPresets.label(32), "3200%")
        XCTAssertEqual(ZoomPresets.label(0.01), "1%")
        XCTAssertEqual(ZoomPresets.label(0.015), "1.5%")
    }
}

final class ViewportTests: XCTestCase {
    func big() -> Viewport {
        var v = Viewport(documentSize: CGSize(width: 6000, height: 4000), viewSize: CGSize(width: 800, height: 600))
        v.setZoom(1)
        v.center(on: CGPoint(x: 3000, y: 2000))
        return v
    }

    func testZoomAroundPointKeepsThatPointUnderTheCursor() {
        for zoom in [0.5, 0.75, 1.37, 2, 8, 32] {
            for cursor in [CGPoint(x: 10, y: 20), CGPoint(x: 400, y: 300), CGPoint(x: 799, y: 1)] {
                var v = big()
                let before = v.documentPoint(atView: cursor)
                v.setZoom(zoom, keeping: cursor)
                let after = v.viewPoint(ofDocument: before)
                XCTAssertEqual(after.x, cursor.x, accuracy: 1e-9, "zoom \(zoom) cursor \(cursor)")
                XCTAssertEqual(after.y, cursor.y, accuracy: 1e-9, "zoom \(zoom) cursor \(cursor)")
            }
        }
    }

    func testZoomAroundPointNearAnEdgeStaysInsideTheScrollLimits() {
        var v = big()
        v.setZoom(0.25, keeping: CGPoint(x: 10, y: 20))
        // 1500x1000 content in an 800x600 view: the origin stays within the limits.
        XCTAssertEqual(v.origin.y, -400)
        XCTAssertEqual(v.origin.x, 10 - 2610 * 0.25)
    }

    func testSmallDocumentIsCenteredAndCannotScroll() {
        var v = Viewport(documentSize: CGSize(width: 100, height: 50), viewSize: CGSize(width: 800, height: 600))
        XCTAssertEqual(v.origin, CGPoint(x: 350, y: 275))
        v.pan(by: CGSize(width: 500, height: -500))
        XCTAssertEqual(v.origin, CGPoint(x: 350, y: 275))
        XCTAssertEqual(v.knobProportion, CGSize(width: 1, height: 1))
    }

    func testScrollingStopsAtTheDocumentEdges() {
        var v = big()
        v.pan(by: CGSize(width: 1e6, height: 1e6))
        XCTAssertEqual(v.origin, .zero)
        XCTAssertEqual(v.scrollFraction, .zero)
        v.pan(by: CGSize(width: -1e7, height: -1e7))
        XCTAssertEqual(v.origin, CGPoint(x: 800 - 6000, y: 600 - 4000))
        XCTAssertEqual(v.scrollFraction, CGPoint(x: 1, y: 1))
    }

    func testScrollFractionRoundTrip() {
        var v = big()
        v.setScrollFraction(x: 0.25, y: 0.75)
        XCTAssertEqual(v.scrollFraction.x, 0.25, accuracy: 1e-12)
        XCTAssertEqual(v.scrollFraction.y, 0.75, accuracy: 1e-12)
        XCTAssertEqual(v.knobProportion.width, 800.0 / 6000, accuracy: 1e-12)
    }

    func testZoomToolClickStepsAndRecentersOnTheClickedPoint() {
        var v = big()
        let click = CGPoint(x: 100, y: 150)
        let target = v.documentPoint(atView: click)
        v.zoomIn(at: click)
        XCTAssertEqual(v.zoom, 2)
        let shown = v.viewPoint(ofDocument: target)
        XCTAssertEqual(shown.x, 400, accuracy: 1e-9)
        XCTAssertEqual(shown.y, 300, accuracy: 1e-9)
        v.zoomOut(at: v.viewCenter)
        XCTAssertEqual(v.zoom, 1)
    }

    func testZoomToRectangleFitsAndCenters() {
        var v = big()
        v.zoom(toDocumentRect: CGRect(x: 1000, y: 1000, width: 198, height: 98))
        XCTAssertEqual(v.zoom, min(800.0 / 200, 600.0 / 100))
        let c = v.viewPoint(ofDocument: CGPoint(x: 1099, y: 1049))
        XCTAssertEqual(c.x, 400, accuracy: 1e-9)
        XCTAssertEqual(c.y, 300, accuracy: 1e-9)
    }

    func testFitAndActualSize() {
        var v = big()
        v.zoomToFit()
        XCTAssertEqual(v.zoom, min(790.0 / 6000, 590.0 / 4000), accuracy: 1e-12)
        XCTAssertEqual(v.origin.x, ((800 - 6000 * v.zoom) / 2).rounded())
        v.actualSize()
        XCTAssertEqual(v.zoom, 1)
    }
}
