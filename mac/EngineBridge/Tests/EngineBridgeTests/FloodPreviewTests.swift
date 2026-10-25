// M31: the live Magic Wand session through the Swift wrapper: begin, two
// updates, the live mask, commit as one "Magic Wand" step, and cancel.
import Foundation
import XCTest

@testable import EngineBridge

private let rampPath = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()
    .appendingPathComponent("tests/fixtures/ramp_256x16.png").path

final class FloodPreviewTests: XCTestCase {
    func testLiveWandSessionCommitsOneStepAndCancelKeepsTheDocument() async throws {
        let engine = try Engine()
        let doc = try engine.openPNG(path: rampPath)
        let params: [String: any Sendable] = ["x": 0, "y": 0, "tolerance": 0.0, "combine": "replace"]
        let first = await engine.result(of: try doc.floodBegin("tool.magic_wand", params))
        XCTAssertEqual(first?.status, "ok")
        XCTAssertEqual(try doc.selectionMask(preview: true).filter { $0 == 255 }.count, 16, "column 0")
        _ = try doc.floodUpdate(["tolerance": 0.25])
        let last = await engine.result(of: try doc.floodUpdate(["tolerance": 0.5]))
        XCTAssertEqual(last?.status, "ok")
        // Gray steps 0 to 74 are inside tolerance 50% with the legacy formula.
        XCTAssertEqual(try doc.selectionMask(preview: true).filter { $0 == 255 }.count, 75 * 16)
        XCTAssertEqual(try doc.selectionPath(preview: true)["active"] as? Bool, true)
        XCTAssertEqual(try doc.selectionPath(preview: false)["active"] as? Bool, false)
        let done = await engine.result(of: try doc.floodFinish())
        XCTAssertEqual(done?.status, "ok")
        let labels = try doc.info()["history_labels"] as? [String] ?? []
        XCTAssertEqual(labels.count, 2)
        XCTAssertEqual(labels.last, "Magic Wand")
        let committed = try doc.selectionMask(preview: false)
        _ = await engine.result(of: try doc.floodBegin("tool.magic_wand", params))
        let cancelled = await engine.result(of: try doc.floodCancel())
        XCTAssertEqual(cancelled?.status, "ok")
        XCTAssertEqual(try doc.selectionMask(preview: false), committed)
        XCTAssertEqual((try doc.info()["history_labels"] as? [String])?.count, 2)
        engine.close()
    }
}
