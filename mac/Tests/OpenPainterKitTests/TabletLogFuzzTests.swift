// M57: the fuzz target for the tablet (pointer) log reader, which is Swift
// (`TabletLog.decode`), so the engine's `cargo xtask fuzz` cannot reach it.
// Random mutations of the recorded log (bit flips, cuts, inserted bytes,
// spliced JSON fragments and edge numbers) must each read or throw within
// 1 s, and a log that reads must replay into stroke samples. The run lasts
// OP_FUZZ_SECONDS (default 5 s in the normal suite; the M57 report ran 600).
import Foundation
@testable import OpenPainterKit
import XCTest

final class TabletLogFuzzTests: XCTestCase {
    struct Rng {
        var s: UInt64
        mutating func next() -> UInt64 {
            s ^= s << 13
            s ^= s >> 7
            s ^= s << 17
            return s
        }
        mutating func below(_ n: Int) -> Int { Int(next() % UInt64(max(n, 1))) }
    }

    static let fragments: [[UInt8]] = [
        "1e308", "-1e308", "NaN", "null", "[]", "{}", "\"", "-0", "1.5", "99999999999999999999",
        "{\"t\":", "]", ",", "\"tilt\":[2,2]", "\"pressure\":-1", "\"buttons\":-3", "\\u0000",
    ].map { Array($0.utf8) }

    static func mutate(_ data: inout [UInt8], _ rng: inout Rng) {
        for _ in 0..<(1 + rng.below(4)) {
            if data.isEmpty {
                data.append(UInt8(truncatingIfNeeded: rng.next()))
                continue
            }
            let p = rng.below(data.count)
            switch rng.below(6) {
            case 0: data[p] ^= UInt8(1 << rng.below(8))
            case 1: data.removeSubrange(p..<min(data.count, p + 1 + rng.below(32)))
            case 2: data.insert(contentsOf: (0..<(1 + rng.below(8))).map { _ in UInt8(truncatingIfNeeded: rng.next()) }, at: p)
            case 3: data.insert(contentsOf: fragments[rng.below(fragments.count)], at: p)
            case 4: data.removeSubrange(p..<data.count)
            default:
                let end = min(data.count, p + 1 + rng.below(64))
                data.insert(contentsOf: Array(data[p..<end]), at: rng.below(data.count))
            }
        }
    }

    func testMutatedTabletLogsReadOrFailCleanlyWithinTheTimeLimit() throws {
        let seed = try Data(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appendingPathComponent("tablet-logs/wave-and-loop.json"))
        XCTAssertNoThrow(try TabletLog.decode(seed))
        let seconds = Double(ProcessInfo.processInfo.environment["OP_FUZZ_SECONDS"] ?? "") ?? 5
        var rng = Rng(s: 0x57_7AB1E7)
        var corpus = [Array(seed)]
        var cases = 0, ok = 0, slowest = 0.0
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            var data = corpus[rng.below(corpus.count)]
            Self.mutate(&data, &rng)
            let start = Date()
            if let log = try? TabletLog.decode(Data(data)) {
                ok += 1
                _ = TabletLog.strokeSamples(log.samples)
                if corpus.count < 64 { corpus.append(data) }
            }
            let took = Date().timeIntervalSince(start)
            slowest = max(slowest, took)
            XCTAssertLessThan(took, 1.0, "case \(cases) took \(took) s")
            cases += 1
        }
        print("TABLETLOG_FUZZ cases \(cases) ok \(ok) crashes 0 slowest \(Int(slowest * 1000)) ms in \(Int(seconds)) s")
        XCTAssertGreaterThan(cases, 100)
    }
}
