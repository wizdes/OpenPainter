import COpPresentSpike
import XCTest
@testable import PresenterSpikeKit

/// Drives both presenter paths through the Rust interface. Each test opens one small
/// window and closes it. Screen moves and sleep/wake are simulated with notifications.
final class PresenterSpikeTests: XCTestCase {
    static var ctx: EngineContext!

    override class func setUp() {
        super.setUp()
        Host.boot()
        ctx = try! EngineContext()
    }

    func runScenario(_ path: String, _ body: (Harness) -> ScenarioResult) throws {
        let h: Harness = path == "A" ? try PathA(ctx: Self.ctx) : try PathB(ctx: Self.ctx)
        defer { h.close() }
        var r = body(h)
        Scenarios.annotate(&r, h)
        print(r)
        XCTAssertTrue(r.passed, r.failures.prefix(8).joined(separator: "\n"))
        XCTAssertGreaterThan(r.iterations, 0)
    }

    func testA_Resizes() throws { try runScenario("A") { Scenarios.resizes($0) } }
    func testB_Resizes() throws { try runScenario("B") { Scenarios.resizes($0) } }
    func testA_DocumentSwitches() throws { try runScenario("A") { Scenarios.documentSwitches($0) } }
    func testB_DocumentSwitches() throws { try runScenario("B") { Scenarios.documentSwitches($0) } }
    func testA_HideShow() throws { try runScenario("A") { Scenarios.hideShow($0) } }
    func testB_HideShow() throws { try runScenario("B") { Scenarios.hideShow($0) } }
    func testA_ScreenMovesSimulated() throws { try runScenario("A") { Scenarios.screenMoves($0) } }
    func testB_ScreenMovesSimulated() throws { try runScenario("B") { Scenarios.screenMoves($0) } }
    func testA_SleepWakeSimulated() throws { try runScenario("A") { Scenarios.sleepWake($0) } }
    func testB_SleepWakeSimulated() throws { try runScenario("B") { Scenarios.sleepWake($0) } }

    func retained(_ path: String) {
        let w = Host.makeWindow(title: "M09 retained-view test")
        defer { w.orderOut(nil); w.close() }
        let r = Scenarios.retained(ctx: Self.ctx, path: path, window: w)
        print(r)
        XCTAssertTrue(r.passed, r.failures.joined(separator: "\n"))
        XCTAssertEqual(r.iterations, 100)
    }

    func testA_RetainedViewGrowth() { retained("A") }
    func testB_RetainedViewGrowth() { retained("B") }

    /// Path B's ring: the engine never writes a slot that is being shown, stale ready
    /// frames go back to the engine, and a wrong release is refused and counted.
    func testB_RingProtocol() throws {
        let h = try PathB(ctx: Self.ctx)
        defer { h.close() }
        try h.setDocument(Charts.chart(0))
        var f = OpPsFrame()
        for _ in 0..<3 { XCTAssertEqual(op_ps_render(h.presenter, &f), 0) }
        let last = f.seq
        var held = OpPsFrame()
        XCTAssertEqual(op_ps_ring_acquire(h.presenter, true, &held), 0)
        XCTAssertEqual(held.seq, last, "acquire returns the newest finished frame")
        for _ in 0..<6 {
            XCTAssertEqual(op_ps_render(h.presenter, &f), 0)
            XCTAssertNotEqual(f.slot, held.slot, "engine wrote the slot being shown")
            var g = OpPsFrame()
            XCTAssertEqual(op_ps_ring_acquire(h.presenter, true, &g), 0)
            XCTAssertNotEqual(g.slot, held.slot)
            XCTAssertEqual(op_ps_ring_release(h.presenter, g.slot), 0)
        }
        // Releasing a slot that is not being shown is refused.
        XCTAssertEqual(op_ps_ring_release(h.presenter, (held.slot + 1) % 3), 2)
        XCTAssertEqual(try h.stats().ring_violations, 1)
        // A new ring is refused while a slot is still shown.
        var ptrs: [UnsafeMutableRawPointer?] = h.surfaces.map { Unmanaged.passUnretained($0).toOpaque() }
        XCTAssertEqual(op_ps_set_ring(h.presenter, &ptrs, 3), 4)
        XCTAssertEqual(op_ps_ring_release(h.presenter, held.slot), 0)
        // With 2 slots both shown, render reports busy instead of overwriting.
        h.ringCount = 2
        var s = h.state
        s.widthPt += 1
        try h.setView(s)
        var a = OpPsFrame(), b = OpPsFrame()
        XCTAssertEqual(op_ps_render(h.presenter, &f), 0)
        XCTAssertEqual(op_ps_ring_acquire(h.presenter, true, &a), 0)
        XCTAssertEqual(op_ps_render(h.presenter, &f), 0)
        XCTAssertEqual(op_ps_ring_acquire(h.presenter, true, &b), 0)
        XCTAssertEqual(op_ps_render(h.presenter, &f), 4, "busy, not overwrite")
        XCTAssertEqual(op_ps_ring_release(h.presenter, a.slot), 0)
        XCTAssertEqual(op_ps_ring_release(h.presenter, b.slot), 0)
    }

    func testPanicIsCaughtAtTheBoundary() {
        XCTAssertEqual(op_ps_debug_panic(), 99)
    }

    func testBadInputsAreRefused() throws {
        let h = try PathA(ctx: Self.ctx)
        defer { h.close() }
        let junk: [UInt8] = [1, 2, 3]
        XCTAssertEqual(op_ps_set_document(h.presenter, junk, 3, 1, 1, junk, 3), 2, "bad ICC and bad size")
        XCTAssertEqual(op_ps_set_display_profile(h.presenter, junk, 3), 2)
        var v = OpPsView(width_pt: 0, height_pt: 10, scale: 1, zoom: 1, origin_x: 0, origin_y: 0)
        XCTAssertEqual(op_ps_set_view(h.presenter, &v), 2)
        var f = OpPsFrame()
        XCTAssertEqual(op_ps_render(h.presenter, &f), 2, "no document yet")
        var out: OpaquePointer?
        let notMetal = CALayer()
        XCTAssertEqual(op_ps_new_layer(Self.ctx.raw, Unmanaged.passUnretained(notMetal).toOpaque(), &out), 2)
    }

    /// A frame-time smoke check at a small size (the full 6000 x 4000 numbers come from
    /// the release host: swift run -c release PresenterSpikeHost bench).
    func testFrameTimeSmoke() throws {
        let doc = Charts.big(width: 1200, height: 800)
        for path in ["A", "B"] {
            let h: Harness = path == "A" ? try PathA(ctx: Self.ctx) : try PathB(ctx: Self.ctx)
            let r = try Bench.run(h, doc: doc, scale: 1, percent: 25, frames: 20, warmup: 3)
            print(r)
            XCTAssertEqual(r.failures, 0)
            h.close()
        }
    }
}
