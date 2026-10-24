// The canvas in a real (small) window: first frame, the display tag from the
// screen, simulated screen and profile changes, simulated sleep and wake, menu
// zoom commands and scroll-bar sync.
import AppKit
import OpenPainterKit
import XCTest

@MainActor
final class CanvasWindowTests: XCTestCase {
    var engine: Engine!
    var window: NSWindow!
    var container: CanvasContainerView!

    override func setUp() async throws {
        _ = NSApplication.shared
        engine = try Engine()
        let doc = try engine.openPNG(path: fixtures.appendingPathComponent("plasma_alpha_512x384.png").path)
        container = CanvasContainerView(document: doc, documentSize: CGSize(width: 512, height: 384))
        window = NSWindow(
            contentRect: NSRect(x: 60, y: 60, width: 420, height: 300), styleMask: [.titled], backing: .buffered,
            defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = container
        window.orderFrontRegardless()
        container.layoutSubtreeIfNeeded()
    }

    override func tearDown() async throws {
        let doc = container.canvas.document
        window.orderOut(nil)
        window.contentView = nil  // detaches the engine view
        window.close()
        try doc.close()
        engine.close()
    }

    func frames(after n: UInt64) throws -> Bool {
        let view = try XCTUnwrap(container.canvas.engineView)
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline {
            if try view.info().frames > n { return true }
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        return false
    }

    func testFirstFrameDisplayTagAndProfileChanges() throws {
        let canvas = container.canvas
        XCTAssertTrue(try frames(after: 0), "first frame")
        let view = try XCTUnwrap(canvas.engineView)
        var info = try view.info()
        let screenICC = (window.screen ?? NSScreen.main)?.colorSpace?.cgColorSpace?.copyICCData() as Data?
        XCTAssertEqual(info.displayICCLength, screenICC?.count, "the screen profile was sent")
        XCTAssertTrue(info.layerTagIsDisplay)
        XCTAssertTrue(info.hasDisplayTable)
        XCTAssertEqual(info.drawableWidth, Int((canvas.bounds.width * window.backingScaleFactor).rounded()))
        XCTAssertEqual(info.state.zoom, ZoomPresets.fit(document: CGSize(width: 512, height: 384), view: canvas.bounds.size),
                       "a new window fits the document")
        // Simulated move to a Display P3 screen.
        let builds = info.tableBuilds
        canvas.displayICCOverride = p3ICC()
        NotificationCenter.default.post(name: NSWindow.didChangeScreenNotification, object: window)
        XCTAssertTrue(try frames(after: info.frames))
        info = try view.info()
        XCTAssertEqual(info.tableBuilds, builds + 1, "table rebuilt for the new display")
        XCTAssertEqual(info.displayICCLength, p3ICC().count)
        XCTAssertEqual(canvas.metalLayer.colorspace?.copyICCData() as Data?, p3ICC())
        // A profile change notification with the same profile does not rebuild.
        NotificationCenter.default.post(name: NSWindow.didChangeScreenProfileNotification, object: window)
        XCTAssertTrue(try frames(after: info.frames))
        XCTAssertEqual(try view.info().tableBuilds, builds + 1)
        // And back to sRGB through the color space notification.
        canvas.displayICCOverride = srgbICC()
        NotificationCenter.default.post(name: NSScreen.colorSpaceDidChangeNotification, object: nil)
        XCTAssertTrue(try frames(after: try view.info().frames))
        XCTAssertEqual(try view.info().tableBuilds, builds + 2)
        XCTAssertEqual(canvas.metalLayer.colorspace?.copyICCData() as Data?, srgbICC())
        XCTAssertEqual(try view.info().frameErrors, 0)
    }

    func testSimulatedSleepAndWake() throws {
        XCTAssertTrue(try frames(after: 0))
        let view = try XCTUnwrap(container.canvas.engineView)
        NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.willSleepNotification, object: nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        var info = try view.info()
        XCTAssertTrue(info.state.suspended)
        XCTAssertTrue(info.suspended, "surface dropped")
        XCTAssertEqual(try engine.viewCounters().liveSurfaces, 0)
        NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.didWakeNotification, object: nil)
        XCTAssertTrue(try frames(after: info.frames))
        info = try view.info()
        XCTAssertFalse(info.suspended)
        XCTAssertEqual(try engine.viewCounters().liveSurfaces, 1)
        XCTAssertEqual(info.frameErrors, 0)
    }

    func testMenuZoomCommandsAndPixelGrid() throws {
        let canvas = container.canvas
        XCTAssertTrue(try frames(after: 0))
        canvas.actualSize(nil)
        XCTAssertEqual(canvas.viewport.zoom, 1)
        canvas.zoomIn(nil)
        XCTAssertEqual(canvas.viewport.zoom, 2)
        canvas.zoomIn(nil)
        canvas.zoomIn(nil)
        XCTAssertEqual(canvas.viewport.zoom, 4)
        canvas.togglePixelGrid(nil)
        XCTAssertTrue(canvas.pixelGrid)
        let view = try XCTUnwrap(canvas.engineView)
        XCTAssertTrue(try view.info().state.pixelGrid)
        XCTAssertEqual(try view.info().state.zoom, 4)
        canvas.zoomOut(nil)
        XCTAssertEqual(canvas.viewport.zoom, 3)
        canvas.zoomToWindow(nil)
        XCTAssertEqual(canvas.viewport.zoom, ZoomPresets.fit(document: CGSize(width: 512, height: 384), view: canvas.bounds.size))
        XCTAssertTrue(try frames(after: try view.info().frames - 1))
        XCTAssertEqual(try view.info().frameErrors, 0)
    }
}
