// Engine views through op_view_*: the state round trip, and 100 attach/detach
// cycles with every counter back to its baseline. These use CAMetalLayers that
// are not in a window.
import OpenPainterKit
import QuartzCore
import XCTest

let fixtures = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().appendingPathComponent("../../../tests/fixtures").standardizedFileURL

func srgbICC() -> Data { CGColorSpace(name: CGColorSpace.sRGB)!.copyICCData()! as Data }
func p3ICC() -> Data { CGColorSpace(name: CGColorSpace.displayP3)!.copyICCData()! as Data }

final class EngineViewTests: XCTestCase {
    var engine: Engine!
    var doc: Document!

    override func setUpWithError() throws {
        engine = try Engine()
        doc = try engine.openPNG(path: fixtures.appendingPathComponent("plasma_alpha_512x384.png").path)
    }

    override func tearDown() {
        try? doc?.close()
        engine?.close()
    }

    func testStateRoundTripThroughOpViewSet() throws {
        let layer = CAMetalLayer()
        let view = try EngineView(document: doc, layer: layer)
        let state = CanvasViewState(
            widthPt: 321, heightPt: 123, scale: 2, zoom: 0.33, originX: -12.5, originY: 7.25, pixelGrid: true)
        let icc = srgbICC()
        try view.set(state, displayICC: icc)
        XCTAssertTrue(try view.waitFrames(1, timeout: .seconds(10)))
        let info = try view.info()
        XCTAssertEqual(info.state, state)
        XCTAssertEqual(info.doc, doc.id)
        XCTAssertEqual(info.displayICCLength, icc.count)
        XCTAssertEqual(info.drawableWidth, 642)
        XCTAssertEqual(info.drawableHeight, 246)
        XCTAssertTrue(info.layerTagIsDisplay)
        XCTAssertTrue(info.hasDisplayTable)
        XCTAssertEqual(info.mipBuilds, 1, "33% reads the mip pyramid")
        XCTAssertEqual(info.frameErrors, 0)
        XCTAssertEqual(layer.colorspace?.copyICCData() as Data?, icc, "the layer carries the display tag")
        // A second state without ICC keeps the profile; grid off, suspended.
        let next = CanvasViewState(widthPt: 50, heightPt: 40, scale: 1, zoom: 4, originX: 0, originY: 0, suspended: true)
        try view.set(next)
        let info2 = try view.info()
        XCTAssertEqual(info2.state, next)
        XCTAssertEqual(info2.displayICCLength, icc.count)
        // Bad states are refused and change nothing.
        var bad = next
        bad.zoom = .nan
        XCTAssertThrowsError(try view.set(bad)) { XCTAssertEqual($0 as? EngineError, .badArgument) }
        XCTAssertThrowsError(try view.set(next, displayICC: Data([1, 2, 3]))) {
            XCTAssertEqual($0 as? EngineError, .badArgument)
        }
        XCTAssertEqual(try view.info().state, next)
        try view.detach()
        XCTAssertThrowsError(try view.info()) { XCTAssertEqual($0 as? EngineError, .noSuchDoc) }
    }

    func testHundredAttachDetachCyclesLeaveNoLeaks() throws {
        func cycle(_ k: Int) throws -> Bool {
            weak var weakLayer: CAMetalLayer?
            try autoreleasepool {
                let layer = CAMetalLayer()
                weakLayer = layer
                let view = try EngineView(document: doc, layer: layer)
                let zoom = [0.25, 1, 4][k % 3]
                try view.set(
                    CanvasViewState(widthPt: 200, heightPt: 150, scale: 2, zoom: zoom, originX: 0, originY: 0),
                    displayICC: k % 2 == 0 ? srgbICC() : p3ICC())
                XCTAssertTrue(try view.waitFrames(1, timeout: .seconds(10)), "cycle \(k)")
                try view.detach()
            }
            return weakLayer == nil
        }
        XCTAssertTrue(try cycle(0), "warm-up layer freed")
        let base = try engine.viewCounters()
        XCTAssertEqual(base.liveViews, 0)
        XCTAssertEqual(base.liveLayers, 0)
        var freed = 0
        for k in 1...100 where try cycle(k) { freed += 1 }
        let after = try engine.viewCounters()
        print("view counters baseline \(base)\nview counters after 100 cycles \(after)")
        XCTAssertEqual(freed, 100, "every CAMetalLayer was freed after detach")
        XCTAssertEqual(after.resources, base.resources)
        XCTAssertEqual(after.validationErrors, 0)
        XCTAssertGreaterThanOrEqual(after.framesTotal, base.framesTotal + 100)
    }

    func testClosingTheDocumentStopsItsView() throws {
        let layer = CAMetalLayer()
        let view = try EngineView(document: doc, layer: layer)
        try view.set(CanvasViewState(widthPt: 100, heightPt: 100, scale: 1, zoom: 1, originX: 0, originY: 0))
        XCTAssertTrue(try view.waitFrames(1, timeout: .seconds(10)))
        try doc.close()
        doc = nil
        XCTAssertEqual(try engine.viewCounters().liveViews, 0)
        XCTAssertThrowsError(try view.info()) { XCTAssertEqual($0 as? EngineError, .noSuchDoc) }
        XCTAssertNoThrow(try view.detach(), "detach after close is quiet")
    }
}
