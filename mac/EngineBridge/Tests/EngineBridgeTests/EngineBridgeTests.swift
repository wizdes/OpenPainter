import Foundation
import XCTest

@testable import EngineBridge

/// The repo root: Tests/EngineBridgeTests/<file> -> mac/EngineBridge -> repo.
private let repoRoot = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()

/// ramp_256x16.png: pixel (x, y) is (x, x, x, 255).
private let rampPath = repoRoot.appendingPathComponent("tests/fixtures/ramp_256x16.png").path
private let ramp = rampBytes(inverted: false)
private let rampInverted = rampBytes(inverted: true)

private func rampBytes(inverted: Bool) -> [UInt8] {
    var out: [UInt8] = []
    out.reserveCapacity(256 * 16 * 4)
    for _ in 0..<16 {
        for x in 0..<256 {
            let v = UInt8(inverted ? 255 - x : x)
            out += [v, v, v, 255]
        }
    }
    return out
}

private func historyCount(_ doc: Document) throws -> Int {
    (try doc.info()["history_labels"] as? [Any])?.count ?? -1
}

/// 1024x512 opaque pixels: 8 tiles of 256x256.
private func bigDocument(_ engine: Engine) throws -> Document {
    var rgba: [UInt8] = []
    rgba.reserveCapacity(1024 * 512 * 4)
    for i in 0..<(1024 * 512) {
        rgba += [UInt8(i % 251), UInt8(i % 241), 7, 255]
    }
    return try engine.importPixels(width: 1024, height: 512, rgba: rgba)
}

final class EngineBridgeTests: XCTestCase {
    func testOpenAddLayerInvertUndoReadsBackExpectedBytes() async throws {
        let engine = try Engine()
        let doc = try engine.openPNG(path: rampPath)
        XCTAssertEqual(try doc.pixels(), ramp)

        let added = try await doc.run("layer.add", ["name": "Top"])
        XCTAssertEqual(added.status, "ok")
        XCTAssertEqual((try doc.info()["layers"] as? [Any])?.count, 2)

        // layer.add puts the new layer on top; the background stays active.
        let inverted = try await doc.run("image.invert_colors")
        XCTAssertEqual(inverted.status, "ok")
        XCTAssertEqual(try doc.pixels(), rampInverted)

        let undone = try await doc.run("edit.undo")
        XCTAssertEqual(undone.status, "ok")
        XCTAssertEqual(try doc.pixels(), ramp)
        let info = try doc.info()
        XCTAssertEqual(info["history_index"] as? Int, 1)
        XCTAssertEqual((info["history_labels"] as? [Any])?.count, 3)

        try doc.close()
        engine.close()
        // The stream ends after close and carried the document events.
        var kinds: [EngineEvent.Kind] = []
        for await event in engine.events { kinds.append(event.kind) }
        XCTAssertTrue(kinds.contains(.docChanged))
        XCTAssertTrue(kinds.contains(.layersChanged))
        XCTAssertEqual(kinds.filter { $0 == .jobFinished }.count, 3)
    }

    func testHundredPreviewUpdatesThenCommitGiveOneHistoryStep() async throws {
        let engine = try Engine()
        let doc = try engine.openPNG(path: rampPath)
        _ = try doc.dispatch("preview.begin", ["command": "image.invert_colors", "params": [String: Int]()])
        for _ in 0..<100 {
            _ = try doc.dispatch("preview.update")
        }
        let committed = try await doc.run("preview.commit")
        XCTAssertEqual(committed.status, "ok", "\(committed.json)")
        XCTAssertEqual(try historyCount(doc), 2)
        XCTAssertEqual(try doc.pixels(), rampInverted)
        engine.close()
    }

    func testPreviewCancelAndTicketCancelLeaveStateUnchanged() async throws {
        let engine = try Engine()
        let doc = try engine.openPNG(path: rampPath)
        let begun = try await doc.run("preview.begin", ["command": "image.invert_colors", "params": [String: Int]()])
        XCTAssertEqual(begun.status, "ok")
        XCTAssertEqual(try doc.pixels(preview: true), rampInverted)
        let cancelled = try await doc.run("preview.cancel")
        XCTAssertEqual(cancelled.status, "ok")
        XCTAssertEqual(try doc.pixels(), ramp)
        XCTAssertEqual(try doc.pixels(preview: true), ramp)
        XCTAssertEqual(try historyCount(doc), 1)

        // Cancel a slow command while it runs: 8 tiles at 100 ms each.
        let big = try bigDocument(engine)
        let before = try big.pixels()
        let ticket = try big.dispatch("test.slow_invert", ["delay_ms": 100])
        try await Task.sleep(for: .milliseconds(150))
        let start = ContinuousClock.now
        try engine.cancel(ticket: ticket)
        let result = await engine.result(of: ticket, timeout: .seconds(3))
        XCTAssertEqual(result?.status, "cancelled")
        XCTAssertLessThan(ContinuousClock.now - start, .milliseconds(500))
        XCTAssertEqual(try historyCount(big), 1)
        XCTAssertEqual(try big.info()["revision"] as? Int, 0)
        XCTAssertTrue(try big.pixels() == before)
        engine.close()
    }

    func testPanicCommandMakesDocumentReadOnly() async throws {
        let engine = try Engine()
        let doc = try engine.openPNG(path: rampPath)
        let panicked = try await doc.run("test.panic")
        XCTAssertEqual(panicked.status, "panic")
        XCTAssertEqual(try doc.info()["read_only"] as? Bool, true)
        let refused = try await doc.run("layer.add")
        XCTAssertEqual(refused.status, "read_only")
        // The last committed snapshot stays readable.
        XCTAssertEqual(try doc.pixels(), ramp)
        XCTAssertEqual(try historyCount(doc), 1)
        engine.close()
    }

    func testEngineFreeAfterCloseWithPendingJobsReturnsAndNoCallbackFiresAfterwards() async throws {
        let engine = try Engine()
        let doc = try engine.openPNG(path: rampPath)
        _ = try doc.dispatch("preview.begin", ["command": "test.slow_invert", "params": ["delay_ms": 200]])
        _ = try doc.dispatch("test.slow_invert", ["delay_ms": 100])
        try doc.close()
        // A second document still has queued work when the engine is freed.
        let other = try engine.openPNG(path: rampPath)
        _ = try other.dispatch("test.slow_invert", ["delay_ms": 100])
        _ = try other.dispatch("preview.begin", ["command": "test.slow_invert", "params": ["delay_ms": 100]])
        let start = ContinuousClock.now
        engine.close()
        XCTAssertLessThan(ContinuousClock.now - start, .seconds(5))
        let delivered = engine.sink.delivered
        XCTAssertGreaterThan(delivered, 0)
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertEqual(engine.sink.lateCallbacks, 0)
        XCTAssertEqual(engine.sink.delivered, delivered)
        XCTAssertThrowsError(try other.dispatch("layer.add")) { error in
            XCTAssertEqual(error as? EngineError, .closed)
        }
    }
}
