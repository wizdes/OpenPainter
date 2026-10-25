// M17: ImageIO import (HEIC, AVIF, Camera RAW) and HEIC/AVIF export.
// HEIC and AVIF fixtures are made here with ImageIO; the Camera RAW samples come
// from tools/fetch-raw-samples.sh (~/Library/Caches/OpenPainter/raw-samples).
// Nothing reads the Photos library or the home folder's pictures.
import CoreGraphics
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

/// Makes small HEIC and AVIF files with ImageIO.
enum ImageIOFixtures {
    static let green: [UInt8] = [0, 200, 0, 255]
    static let red: [UInt8] = [220, 30, 30, 255]

    /// 64 x 32 stored pixels: red, with a 16 x 16 green block at the stored top-left.
    static func pattern(width: Int = 64, height: Int = 32) -> [UInt8] {
        var px = [UInt8](repeating: 0, count: width * height * 4)
        for y in 0..<height {
            for x in 0..<width {
                let c = (x < 16 && y < 16) ? green : red
                px.replaceSubrange((y * width + x) * 4..<(y * width + x) * 4 + 4, with: c)
            }
        }
        return px
    }

    static func directory() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("OpenPainterM17-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// Writes `type` ("public.heic" or "public.avif") with an optional EXIF
    /// orientation, color space and GPS/EXIF metadata.
    static func write(
        _ type: String, to url: URL, width: Int = 64, height: Int = 32, orientation: Int = 1,
        space: CFString = CGColorSpace.sRGB, gps: Bool = false
    ) throws {
        let px = pattern(width: width, height: height)
        let icc = CGColorSpace(name: space)!.copyICCData()! as Data
        let image = try ImageIOExporter.cgImage(rgba: px, width: width, height: height, icc: icc)
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, type as CFString, 1, nil))
        var props: [CFString: Any] = [
            kCGImagePropertyOrientation: orientation,
            kCGImageDestinationLossyCompressionQuality: 0.95,
            kCGImagePropertyExifDictionary: [kCGImagePropertyExifUserComment: "M17 fixture"],
        ]
        if gps {
            props[kCGImagePropertyGPSDictionary] = [
                kCGImagePropertyGPSLatitude: 47.5, kCGImagePropertyGPSLatitudeRef: "N",
                kCGImagePropertyGPSLongitude: 122.25, kCGImagePropertyGPSLongitudeRef: "W",
            ]
        }
        CGImageDestinationAddImage(dest, image, props as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(dest), "ImageIO wrote \(url.lastPathComponent)")
        if type == "public.avif", orientation != 1 { try setAVIFRotation(url, orientation: orientation) }
    }

    /// ImageIO's AVIF writer always stores orientation 1 (an `irot` box with
    /// angle 0), so the fixture sets the HEIF `irot` angle itself (ISO/IEC
    /// 23008-12 6.5.10: units of 90 degrees counterclockwise). Only the pure
    /// rotations 3, 6 and 8 exist without a mirror box.
    static func setAVIFRotation(_ url: URL, orientation: Int) throws {
        let angle: UInt8 = try XCTUnwrap([3: 2, 6: 3, 8: 1][orientation], "rotation-only orientation")
        var bytes = [UInt8](try Data(contentsOf: url))
        let tag = Array("irot".utf8)
        let at = try XCTUnwrap((4..<(bytes.count - 5)).first { Array(bytes[$0..<$0 + 4]) == tag && bytes[$0 - 1] == 9 },
                               "the AVIF has a 9-byte irot box")
        bytes[at + 4] = angle
        try Data(bytes).write(to: url)
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        XCTAssertEqual((props?[kCGImagePropertyOrientation as String] as? NSNumber)?.intValue, orientation,
                       "ImageIO reads the patched AVIF orientation")
    }

    static func pixel(_ rgba: [UInt8], width: Int, x: Int, y: Int) -> [UInt8] {
        Array(rgba[(y * width + x) * 4..<(y * width + x) * 4 + 4])
    }

    /// True when every channel is within `tolerance`.
    static func near(_ a: [UInt8], _ b: [UInt8], _ tolerance: Int = 12) -> Bool {
        zip(a, b).allSatisfy { abs(Int($0) - Int($1)) <= tolerance }
    }
}

/// The three Camera RAW samples in tools/raw-samples.txt, with the size and
/// profile ImageIO gives on macOS 27 (RAW decoding can change between macOS versions).
struct RawSample {
    let file: String
    let camera: String
    let width: Int
    let height: Int
    let profile: String

    static let all = [
        RawSample(file: "Nikon-D2H.NEF", camera: "Nikon D2H", width: 2464, height: 1632, profile: "Display P3"),
        RawSample(file: "Panasonic-DMC-LX7.RW2", camera: "Panasonic DMC-LX7", width: 1368, height: 1368,
                  profile: "Display P3"),
        RawSample(file: "Sony-ILCE-7S.ARW", camera: "Sony ILCE-7S", width: 2768, height: 1848, profile: "Display P3"),
    ]

    static var directory: URL {
        if let d = ProcessInfo.processInfo.environment["OPENPAINTER_RAW_SAMPLES"] { return URL(fileURLWithPath: d) }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Caches/OpenPainter/raw-samples")
    }
}

final class ImageIOImportTests: XCTestCase {
    var engine: Engine!
    var dir: URL!
    var docs: [Document] = []

    override func setUpWithError() throws {
        engine = try Engine()
        dir = try ImageIOFixtures.directory()
    }

    override func tearDownWithError() throws {
        for d in docs { try? d.close() }
        engine.close()
        try? FileManager.default.removeItem(at: dir)
    }

    private func open(_ url: URL) throws -> LoadedImage {
        let loaded = try DocumentLoader.open(url, engine: engine)
        docs.append(loaded.document)
        return loaded
    }

    private func profileDescription(_ doc: Document) throws -> String {
        try doc.info()["profile_description"] as? String ?? ""
    }

    private func requireAVIF() throws {
        guard ImageFormats.canEncodeAVIF else {
            throw XCTSkip("This macOS cannot encode AVIF, so no AVIF fixture can be made here.")
        }
    }

    // MARK: Formats

    func testImportTypesCoverHEICAVIFAndEveryRAWTypeImageIOReads() {
        let ids = Set(ImageFormats.imageIOImportTypes().map(\.identifier))
        let system = ImageFormats.systemSourceTypes
        for t in ["public.heic", "public.heif", "public.avif"] where system.contains(t) {
            XCTAssertTrue(ids.contains(t), t)
        }
        let raw = system.filter { UTType($0)?.conforms(to: .rawImage) == true }
        XCTAssertFalse(raw.isEmpty, "ImageIO lists Camera RAW types")
        XCTAssertEqual(ids.intersection(raw).count, raw.count, "every RAW type ImageIO reads is imported")
        XCTAssertFalse(ids.contains("public.jpeg"), "the engine decodes JPEG (M16), not ImageIO")
    }

    // MARK: HEIC and AVIF fixtures

    func testHEICOpensWithSizeProfileAndMetadata() throws {
        let url = dir.appendingPathComponent("plain.heic")
        try ImageIOFixtures.write("public.heic", to: url, gps: true)
        XCTAssertEqual(ImageFormats.route(for: url), .imageIO("public.heic"))
        let loaded = try open(url)
        XCTAssertEqual(loaded.pixelSize, CGSize(width: 64, height: 32))
        XCTAssertEqual(try profileDescription(loaded.document), "sRGB IEC61966-2.1")
        let px = try loaded.document.pixels()
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 64, x: 4, y: 4), ImageIOFixtures.green))
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 64, x: 60, y: 28), ImageIOFixtures.red))
        XCTAssertTrue(loaded.notes.isEmpty, "an 8-bit HEIC needs no note: \(loaded.notes)")
        // Metadata stays with the Swift document (the engine has no slot).
        XCTAssertNotNil(loaded.metadata.xmp)
        let lat = (loaded.metadata.gps?[kCGImagePropertyGPSLatitude as String] as? NSNumber)?.doubleValue
        XCTAssertEqual(lat ?? 0, 47.5, accuracy: 0.001)
        XCTAssertEqual(loaded.metadata.exif?[kCGImagePropertyExifUserComment as String] as? String, "M17 fixture")
    }

    func testHEICOrientation6IsTurnedClockwise() throws {
        let url = dir.appendingPathComponent("orient6.heic")
        try ImageIOFixtures.write("public.heic", to: url, orientation: 6)
        let loaded = try open(url)
        // Stored 64 x 32 turns into 32 x 64; the stored top-left block is now top-right.
        XCTAssertEqual(loaded.pixelSize, CGSize(width: 32, height: 64))
        let px = try loaded.document.pixels()
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 32, x: 28, y: 4), ImageIOFixtures.green))
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 32, x: 4, y: 4), ImageIOFixtures.red))
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 32, x: 28, y: 60), ImageIOFixtures.red))
        // The kept metadata says orientation 1: the pixels are already turned.
        let tiff = loaded.metadata.properties[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
        if let o = tiff?[kCGImagePropertyTIFFOrientation as String] as? NSNumber { XCTAssertEqual(o.intValue, 1) }
    }

    func testHEICDisplayP3ProfileIsKept() throws {
        let url = dir.appendingPathComponent("p3.heic")
        try ImageIOFixtures.write("public.heic", to: url, space: CGColorSpace.displayP3)
        let image = try ImageIOImporter.decode(url: url)
        XCTAssertEqual(image.colorSpaceName, CGColorSpace.displayP3 as String)
        let loaded = try open(url)
        XCTAssertEqual(try profileDescription(loaded.document), "Display P3")
        let icc = try loaded.document.queryData("doc.profile")
        XCTAssertEqual(icc, image.icc, "the engine holds the ICC bytes ImageIO reported")
        XCTAssertEqual(CGColorSpace(iccData: icc as CFData)?.name, CGColorSpace.displayP3)
    }

    func testAVIFOpensWithOrientationAndProfile() throws {
        try requireAVIF()
        let plain = dir.appendingPathComponent("plain.avif")
        try ImageIOFixtures.write("public.avif", to: plain)
        XCTAssertEqual(ImageFormats.route(for: plain), .imageIO("public.avif"))
        let a = try open(plain)
        XCTAssertEqual(a.pixelSize, CGSize(width: 64, height: 32))
        XCTAssertEqual(try profileDescription(a.document), "sRGB IEC61966-2.1")

        let turned = dir.appendingPathComponent("orient6-p3.avif")
        try ImageIOFixtures.write("public.avif", to: turned, orientation: 6, space: CGColorSpace.displayP3)
        let b = try open(turned)
        XCTAssertEqual(b.pixelSize, CGSize(width: 32, height: 64))
        let px = try b.document.pixels()
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 32, x: 28, y: 4), ImageIOFixtures.green))
        XCTAssertTrue(ImageIOFixtures.near(ImageIOFixtures.pixel(px, width: 32, x: 4, y: 4), ImageIOFixtures.red))
        XCTAssertEqual(try profileDescription(b.document), "Display P3")
    }

    func testAllEightOrientationsPutTheStoredCornerInTheRightPlace() throws {
        // Stored 3 x 2: the top-left pixel is marked. Where it lands after each
        // EXIF orientation (TIFF 6.0 / EXIF 2.32 Orientation tag).
        var px = [UInt8](repeating: 0, count: 3 * 2 * 4)
        px[0] = 255
        let expected: [Int: (w: Int, h: Int, x: Int, y: Int)] = [
            1: (3, 2, 0, 0), 2: (3, 2, 2, 0), 3: (3, 2, 2, 1), 4: (3, 2, 0, 1),
            5: (2, 3, 0, 0), 6: (2, 3, 1, 0), 7: (2, 3, 1, 2), 8: (2, 3, 0, 2),
        ]
        for o in 1...8 {
            let (out, w, h) = try ImageIOImporter.orient(px, width: 3, height: 2, orientation: o)
            let e = expected[o]!
            XCTAssertEqual(w, e.w, "orientation \(o) width")
            XCTAssertEqual(h, e.h, "orientation \(o) height")
            XCTAssertEqual(out[(e.y * w + e.x) * 4], 255, "orientation \(o) marked corner")
            XCTAssertEqual(out.enumerated().filter { $0.offset % 4 == 0 && $0.element == 255 }.count, 1)
        }
    }

    func testSixteenBitSourceIsStoredAs8BitWithANote() throws {
        // A 16-bit TIFF in memory runs the same ImageIO conversion path.
        let w = 8, h = 4
        var px16 = [UInt16](repeating: 0, count: w * h * 4)
        for i in stride(from: 0, to: px16.count, by: 4) {
            px16[i] = 65535
            px16[i + 1] = 32768
            px16[i + 3] = 65535
        }
        let data = px16.withUnsafeBufferPointer { Data(buffer: $0) }
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue | CGBitmapInfo.byteOrder16Little.rawValue)
        let image = try XCTUnwrap(CGImage(
            width: w, height: h, bitsPerComponent: 16, bitsPerPixel: 64, bytesPerRow: w * 8, space: space,
            bitmapInfo: info, provider: CGDataProvider(data: data as CFData)!, decode: nil,
            shouldInterpolate: false, intent: .defaultIntent))
        let out = NSMutableData()
        let dest = try XCTUnwrap(CGImageDestinationCreateWithData(out, "public.tiff" as CFString, 1, nil))
        CGImageDestinationAddImage(dest, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        let decoded = try ImageIOImporter.decode(data: out as Data, name: "16bit.tiff")
        XCTAssertEqual(decoded.sourceBitsPerComponent, 16)
        XCTAssertEqual(decoded.rgba.count, w * h * 4)
        XCTAssertTrue(ImageIOFixtures.near(Array(decoded.rgba[0..<4]), [255, 128, 0, 255], 1))
        XCTAssertEqual(decoded.notes.count, 1)
        XCTAssertTrue(decoded.notes[0].contains("16-bit"), decoded.notes[0])
    }

    func testUnsupportedFilesDoNotOpen() throws {
        let text = dir.appendingPathComponent("notes.txt")
        try Data("hello".utf8).write(to: text)
        let fake = dir.appendingPathComponent("fake.heic")
        try Data("not an image".utf8).write(to: fake)
        for url in [text, fake] {
            XCTAssertEqual(ImageFormats.route(for: url), .unsupported)
            XCTAssertThrowsError(try open(url)) { e in
                XCTAssertEqual(e as? DocumentLoaderError, .unsupported(url.lastPathComponent))
            }
        }
    }

    // MARK: Camera RAW samples

    func testCameraRAWSamplesOpenWithSizeOrientationAndProfile() throws {
        var missing: [String] = []
        var opened = 0
        for s in RawSample.all {
            let url = RawSample.directory.appendingPathComponent(s.file)
            guard FileManager.default.fileExists(atPath: url.path) else {
                missing.append(s.file)
                continue
            }
            XCTAssertEqual(ImageFormats.route(for: url).isImageIO, true, s.file)
            let image = try ImageIOImporter.decode(url: url)
            XCTAssertEqual(image.sourceOrientation, 1, "\(s.camera) orientation")
            XCTAssertTrue(UTType(image.sourceType)?.conforms(to: .rawImage) ?? false, image.sourceType)
            XCTAssertTrue(image.notes.contains { $0.contains("16-bit") }, "\(s.camera): \(image.notes)")
            let loaded = try open(url)
            XCTAssertEqual(loaded.pixelSize, CGSize(width: s.width, height: s.height), s.camera)
            XCTAssertEqual(try profileDescription(loaded.document), s.profile, s.camera)
            XCTAssertNotNil(loaded.metadata.exif, "\(s.camera) EXIF kept")
            try loaded.document.close()
            docs.removeAll { $0 === loaded.document }
            opened += 1
        }
        if opened == 0 {
            let reason = "Camera RAW samples missing in \(RawSample.directory.path) (\(missing.joined(separator: ", "))); "
                + "run tools/fetch-raw-samples.sh"
            print("SKIP RAW: \(reason)")
            throw XCTSkip(reason)
        }
        if !missing.isEmpty { print("SKIP RAW (some): missing \(missing.joined(separator: ", "))") }
    }
}

extension ImageFormats.Route {
    var isImageIO: Bool { if case .imageIO = self { true } else { false } }
}

final class ImageIOExportTests: XCTestCase {
    var engine: Engine!
    var dir: URL!

    override func setUpWithError() throws {
        engine = try Engine()
        dir = try ImageIOFixtures.directory()
    }

    override func tearDownWithError() throws {
        engine.close()
        try? FileManager.default.removeItem(at: dir)
    }

    func testAVIFIsOfferedExactlyWhenTheSystemCanEncodeIt() throws {
        let system = ImageFormats.systemDestinationTypes
        let offered = ImageFormats.exportTypes().map(\.identifier)
        XCTAssertEqual(offered.contains("public.avif"), system.contains("public.avif"))
        XCTAssertEqual(offered.contains("public.heic"), system.contains("public.heic"))
        XCTAssertEqual(ImageFormats.canEncodeAVIF, system.contains("public.avif"))
        // Both branches, whatever this Mac can do.
        XCTAssertEqual(ImageFormats.exportTypes(destinationTypes: ["public.heic", "public.png"]).map(\.identifier),
                       ["public.heic"])
        XCTAssertEqual(ImageFormats.exportTypes(destinationTypes: ["public.heic", "public.avif"]).map(\.identifier),
                       ["public.heic", "public.avif"])
        XCTAssertThrowsError(try ImageIOExporter.encode(
            rgba: [0, 0, 0, 255], width: 1, height: 1, icc: nil, type: "public.avif",
            destinationTypes: ["public.heic"])) { e in
            XCTAssertEqual(e as? ImageIOExportError, .unsupportedType("AVIF"))
        }
        print("M17 export types on this Mac: \(offered)")
    }

    /// Round trip: Display P3 HEIC -> engine -> HEIC (quality 0.9) -> decode.
    /// Tolerance: same size and profile; in flat areas every channel within 12
    /// of the first decode, and a mean absolute difference of at most 3 over
    /// all channels (lossy HEVC with 4:2:0 color).
    func testHEICRoundTripKeepsSizeProfileAndMetadata() throws {
        let src = dir.appendingPathComponent("src-p3.heic")
        try ImageIOFixtures.write("public.heic", to: src, orientation: 6, space: CGColorSpace.displayP3, gps: true)
        let first = try DocumentLoader.open(src, engine: engine)
        defer { try? first.document.close() }
        let data = try DocumentLoader.export(first.document, type: "public.heic", metadata: first.metadata)
        let out = dir.appendingPathComponent("out.heic")
        try data.write(to: out)
        let second = try ImageIOImporter.decode(url: out)
        XCTAssertEqual(second.sourceType, "public.heic")
        XCTAssertEqual(second.width, Int(first.pixelSize.width))
        XCTAssertEqual(second.height, Int(first.pixelSize.height))
        XCTAssertEqual(second.sourceOrientation, 1, "pixels were turned on import, so the file says 1")
        XCTAssertEqual(second.colorSpaceName, CGColorSpace.displayP3 as String)
        let a = try first.document.pixels(), b = second.rgba
        XCTAssertEqual(a.count, b.count)
        // HEVC stores color at half resolution (4:2:0), so the red/green edge
        // smears; the worst-channel limit applies 3 px or more away from it.
        let w = second.width, h = second.height
        func flat(_ x: Int, _ y: Int) -> Bool {
            let c = ImageIOFixtures.pixel(a, width: w, x: x, y: y)
            for dy in -3...3 {
                for dx in -3...3 {
                    let (nx, ny) = (x + dx, y + dy)
                    guard nx >= 0, ny >= 0, nx < w, ny < h else { continue }
                    if !ImageIOFixtures.near(ImageIOFixtures.pixel(a, width: w, x: nx, y: ny), c, 2) { return false }
                }
            }
            return true
        }
        var worst = 0, worstEdge = 0, sum = 0
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                let d = (0..<4).map { abs(Int(a[i + $0]) - Int(b[i + $0])) }.max()!
                sum += (0..<4).map { abs(Int(a[i + $0]) - Int(b[i + $0])) }.reduce(0, +)
                if flat(x, y) { worst = max(worst, d) } else { worstEdge = max(worstEdge, d) }
            }
        }
        let mean = Double(sum) / Double(a.count)
        print("M17 HEIC round trip: worst \(worst) away from edges, \(worstEdge) at edges, mean \(String(format: "%.3f", mean))")
        XCTAssertLessThanOrEqual(worst, 12, "flat areas")
        XCTAssertLessThanOrEqual(mean, 3)
        let lat = (second.metadata.gps?[kCGImagePropertyGPSLatitude as String] as? NSNumber)?.doubleValue
        XCTAssertEqual(lat ?? 0, 47.5, accuracy: 0.001, "GPS kept for M50")
    }

    func testPNGDocumentExportsAsHEICAndAVIF() throws {
        let png = fixtures.appendingPathComponent("plasma_alpha_512x384.png")
        let loaded = try DocumentLoader.open(png, engine: engine)
        defer { try? loaded.document.close() }
        for type in ImageFormats.exportTypes().map(\.identifier) {
            let data = try DocumentLoader.export(loaded.document, type: type)
            let back = try ImageIOImporter.decode(data: data, name: type)
            XCTAssertEqual(back.sourceType, type)
            XCTAssertEqual(back.width, 512, type)
            XCTAssertEqual(back.height, 384, type)
        }
    }
}
