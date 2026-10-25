// M57 review item 11: the fuzz target for the ImageIO import path
// (`DocumentLoader.open`, as File > Open uses it). Seeds are HEIC and AVIF
// files this Mac's ImageIO encodes; each input is a mutated copy written to a
// file and opened on its own thread with a deadline: past 5 s a case is
// reported as a stall, past 30 s the test fails as a hang (a mutated size
// field can make a 25-megapixel image from a few hundred bytes, which this
// Debug build opens in 1 to 2 s, and ImageIO's HEVC decoder waits about 6 s
// on some damaged HEIC files), with the process's
// memory footprint under 2 GiB. RAW: this Mac's ImageIO cannot write RAW and
// the repo has no RAW samples (tools/fetch-raw-samples.sh downloads them), so
// RAW files in `OP_FUZZ_RAW_DIR` are used when set. The run lasts
// OP_FUZZ_SECONDS per format (default 3 s in the normal suite).
import AppKit
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

final class ImageIOFuzzTests: XCTestCase {
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

    static func encode(_ type: UTType) -> Data? {
        let (w, h) = (64, 48)
        var px = [UInt8](repeating: 0, count: w * h * 4)
        for i in stride(from: 0, to: px.count, by: 4) {
            px[i] = UInt8(i % 251)
            px[i + 1] = UInt8((i / 7) % 256)
            px[i + 2] = 200
            px[i + 3] = 255
        }
        let provider = CGDataProvider(data: Data(px) as CFData)!
        guard let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { return nil }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, type.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.8] as CFDictionary)
        return CGImageDestinationFinalize(dest) ? out as Data : nil
    }

    static func mutate(_ data: inout [UInt8], _ rng: inout Rng) {
        for _ in 0..<(1 + rng.below(4)) {
            if data.isEmpty {
                data.append(UInt8(truncatingIfNeeded: rng.next()))
                continue
            }
            let p = rng.below(data.count)
            switch rng.below(6) {
            case 0: data[p] ^= UInt8(1 << rng.below(8))
            case 1: data[p] = [0, 1, 0x7F, 0x80, 0xFF][rng.below(5)]
            case 2: data.removeSubrange(p..<min(data.count, p + 1 + rng.below(32)))
            case 3: data.insert(contentsOf: (0..<(1 + rng.below(8))).map { _ in UInt8(truncatingIfNeeded: rng.next()) }, at: p)
            case 4:
                // An edge-case 32-bit size field (box lengths in HEIF and AVIF).
                let v: UInt32 = [0, 1, 8, 0x7FFF_FFFF, 0xFFFF_FFFF][rng.below(5)]
                for (k, b) in withUnsafeBytes(of: v.bigEndian, Array.init).enumerated() where p + k < data.count {
                    data[p + k] = b
                }
            default:
                let end = min(data.count, p + 1 + rng.below(64))
                data.insert(contentsOf: Array(data[p..<end]), at: rng.below(data.count))
            }
        }
    }

    func fuzz(_ name: String, ext: String, seeds: [Data], engine: Engine, seconds: Double) throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("op-imageio-fuzz-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        var rng = Rng(s: 0x57_1A6E_10 ^ UInt64(name.utf8.count))
        var cases = 0, opened = 0, slowest = 0.0, overOneSecond = 0
        let deadline = Date().addingTimeInterval(seconds)
        let watch = try FuzzLimits.MemoryWatch()
        defer { watch.stop() }
        var stalls = 0
        while Date() < deadline {
            var data = Array(seeds[rng.below(seeds.count)])
            Self.mutate(&data, &rng)
            let url = dir.appendingPathComponent("case\(cases % 8).\(ext)")
            try Data(data).write(to: url)
            let start = Date()
            // ImageIO's HEVC decoder can wait about 6 s on some damaged files
            // (tests/fuzz-found/heic-imageio-6s-stall.bin) with the CPU idle;
            // past 5 s a case counts as a stall, past 30 s as a hang.
            guard let ok = FuzzLimits.run(Data(data), deadline: 30.0, { _ -> Bool in
                guard let loaded = try? DocumentLoader.open(url, engine: engine) else { return false }
                try? loaded.document.close()
                return true
            }) else {
                XCTFail("\(name) case \(cases) passed the 30 s limit; input saved to \(FuzzLimits.save(Data(data), name))")
                return
            }
            if ok { opened += 1 }
            let took = Date().timeIntervalSince(start)
            slowest = max(slowest, took)
            if took > 1 { overOneSecond += 1 }
            if took > 5 {
                stalls += 1
                print("IMAGEIO_FUZZ \(name) stall \(Int(took * 1000)) ms: \(FuzzLimits.save(Data(data), name + "-stall"))")
            }
            let grown = try watch.grown()
            guard grown < FuzzLimits.memoryCap else {
                XCTFail("\(name) case \(cases) grew memory by \(grown >> 20) MiB")
                return
            }
            cases += 1
        }
        print("IMAGEIO_FUZZ \(name) cases \(cases) opened \(opened) crashes 0 hangs 0 stalls-over-5s \(stalls) over-1s \(overOneSecond) slowest \(Int(slowest * 1000)) ms in \(Int(seconds)) s")
        XCTAssertGreaterThan(cases, 10, name)
    }

    func testMutatedHEICAVIFAndRAWFilesOpenOrFailCleanlyWithinTheLimits() throws {
        let engine = try Engine()
        defer { engine.close() }
        let seconds = Double(ProcessInfo.processInfo.environment["OP_FUZZ_SECONDS"] ?? "") ?? 3
        let heic = try XCTUnwrap(Self.encode(.heic), "this Mac encodes HEIC")
        XCTAssertNoThrow(try {
            let url = FileManager.default.temporaryDirectory.appendingPathComponent("seed-\(UUID().uuidString).heic")
            try heic.write(to: url)
            defer { try? FileManager.default.removeItem(at: url) }
            try DocumentLoader.open(url, engine: engine).document.close()
        }(), "the HEIC seed opens")
        try fuzz("heic", ext: "heic", seeds: [heic], engine: engine, seconds: seconds)
        if let avifType = UTType("public.avif"), let avif = Self.encode(avifType) {
            try fuzz("avif", ext: "avif", seeds: [avif], engine: engine, seconds: seconds)
        } else {
            print("IMAGEIO_FUZZ avif: this Mac's ImageIO cannot encode AVIF; no seed")
        }
        if let rawDir = ProcessInfo.processInfo.environment["OP_FUZZ_RAW_DIR"] {
            let files = try FileManager.default.contentsOfDirectory(atPath: rawDir).sorted()
            let seeds = try files.prefix(4).map { try Data(contentsOf: URL(fileURLWithPath: rawDir).appendingPathComponent($0)) }
            let ext = (files.first as NSString?)?.pathExtension ?? "dng"
            if !seeds.isEmpty { try fuzz("raw", ext: ext, seeds: seeds, engine: engine, seconds: seconds) }
        } else {
            print("IMAGEIO_FUZZ raw: skipped (no RAW samples; set OP_FUZZ_RAW_DIR)")
        }
    }
}
