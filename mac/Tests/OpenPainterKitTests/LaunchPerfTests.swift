// M58: launch stays lazy. Opening a document up to its first canvas frame (what
// OP_READY marks at launch) makes the view pipeline and no effect kernel; the
// first effect compiles its kernels then. The Release app is checked the same
// way at real launch by scripts/launch-time.sh (the OP_PIPELINES line).
import AppKit
import EngineBridge
@testable import OpenPainterKit
import XCTest

@MainActor
final class LaunchPerfTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws { fx = try ShellFixture() }
    override func tearDown() async throws { fx.tearDown() }

    func testFirstCanvasFrameCompilesNoEffectKernel() async throws {
        let before = try PipelineCounts.now()
        let wc = try fx.open()
        let canvas = wc.canvas
        XCTAssertTrue(waitUntil(20) { (try? canvas.engineView?.info().frames ?? 0) ?? 0 > 0 }, "first frame")
        let ready = try PipelineCounts.now()
        XCTAssertEqual(ready.effect, before.effect, "no effect kernel before the canvas is ready")
        XCTAssertEqual(ready.view, before.view + 1, "the canvas makes its own view pipeline")

        // The first effect compiles its kernels (on the GPU path).
        let result = try await LaunchProbe.runFirstEffect(engine: fx.engine)
        let after = try PipelineCounts.now()
        if result.gpu {
            XCTAssertGreaterThan(after.effect, ready.effect, "the first GPU effect compiles a kernel")
        } else {
            XCTFail("the probe blur ran on the CPU; this Mac has a GPU, so the GPU path should run")
        }
        XCTAssertGreaterThan(result.ms, 0)
    }

    func testProbeRunsOnlyWhenAsked() {
        XCTAssertFalse(LaunchProbe.wantsFirstEffect([:]))
        XCTAssertFalse(LaunchProbe.wantsFirstEffect(["OP_LAUNCH_PROBE": "1"]))
        XCTAssertTrue(LaunchProbe.wantsFirstEffect(["OP_LAUNCH_PROBE": "first-effect"]))
    }
}
