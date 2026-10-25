// M50 save options and metadata: lossless settings keep every pixel byte;
// lossy settings get smaller as quality goes down and stay above a stated
// PSNR; keep, strip-location and strip-all metadata (read back with ImageIO
// and by scanning the raw bytes); the resolution in saved files; options kept
// per format; the size preview shows only the last result; and the options
// sheet in the Save As / Save / Save a Copy flow.
import AppKit
import ImageIO
@testable import OpenPainterKit
import XCTest

/// Raw-byte checks for location metadata.
enum MetadataScan {
    /// GPS IFD pointers (tag 34853) found in any TIFF structure in `data`
    /// (EXIF blocks, TIFF files and embedded thumbnails), and how many of
    /// those structures have more than one IFD (a thumbnail's IFD1).
    static func tiffStructures(_ data: Data) -> (gpsPointers: Int, structures: Int, withIFD1: Int) {
        let b = [UInt8](data)
        var gps = 0, found = 0, ifd1 = 0
        var i = 0
        while i + 8 <= b.count {
            let le = b[i] == 0x49 && b[i + 1] == 0x49 && b[i + 2] == 42 && b[i + 3] == 0
            let be = b[i] == 0x4D && b[i + 1] == 0x4D && b[i + 2] == 0 && b[i + 3] == 42
            if le || be, let (g, n) = walk(b, base: i, le: le) {
                found += 1
                gps += g
                if n > 1 { ifd1 += 1 }
            }
            i += 1
        }
        return (gps, found, ifd1)
    }

    /// Walks the IFD chain of the TIFF structure at `base`: (GPS pointers, IFD count).
    static func walk(_ b: [UInt8], base: Int, le: Bool) -> (Int, Int)? {
        func u16(_ p: Int) -> Int? {
            guard p >= 0, p + 2 <= b.count else { return nil }
            return le ? Int(b[p]) | Int(b[p + 1]) << 8 : Int(b[p]) << 8 | Int(b[p + 1])
        }
        func u32(_ p: Int) -> Int? {
            guard let hi = u16(le ? p + 2 : p), let lo = u16(le ? p : p + 2) else { return nil }
            return hi << 16 | lo
        }
        guard var off = u32(base + 4), off >= 8 else { return nil }
        var gps = 0, ifds = 0
        while off != 0, ifds < 4 {
            let p = base + off
            guard let n = u16(p), n > 0, n < 512, p + 2 + n * 12 + 4 <= b.count else { return ifds > 0 ? (gps, ifds) : nil }
            for e in 0..<n where u16(p + 2 + e * 12) == 34853 { gps += 1 }
            ifds += 1
            guard let next = u32(p + 2 + n * 12) else { break }
            off = next
        }
        return ifds > 0 ? (gps, ifds) : nil
    }

    /// The XMP and EXIF names of location fields found as text in `data`.
    static func gpsNames(_ data: Data) -> [String] {
        let names = ["exif:GPS", "GPSLatitude", "GPSLongitude", "GPSVersionID"]
        return names.filter { data.range(of: Data($0.utf8)) != nil }
    }
}

@MainActor
final class SaveOptionsTests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var docs: [ImageDocument] = []
    var opened: [Document] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedPrompt: ImageDocument.SaveOptionsPrompt!
    var savedFlatten: ImageDocument.FlattenPrompt!
    var savedStore: SaveOptionsStore!
    var suite: String!
    var prompts: [(type: String, initial: SaveOptions)] = []
    var promptAnswers: [SaveOptions?] = []

    static let lat = 47.5
    static let lon = 122.25

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedPrompt = ImageDocument.saveOptionsPrompt
        savedFlatten = ImageDocument.flattenPrompt
        savedStore = ImageDocument.saveOptionsStore
        suite = "OpenPainterM50.\(UUID().uuidString)"
        ImageDocument.saveOptionsStore = SaveOptionsStore(defaults: UserDefaults(suiteName: suite)!)
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        ImageDocument.flattenPrompt = { _, _, done in done(true) }
        ImageDocument.saveOptionsPrompt = { [unowned self] _, type, initial, done in
            prompts.append((type, initial))
            done(promptAnswers.isEmpty ? initial : promptAnswers.removeFirst())
        }
    }

    override func tearDown() async throws {
        for d in docs {
            for wc in d.windowControllers { wc.window?.orderOut(nil) }
            d.close()
        }
        docs.removeAll()
        for d in opened { try? d.close() }
        opened.removeAll()
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        ImageDocument.saveOptionsPrompt = savedPrompt
        ImageDocument.flattenPrompt = savedFlatten
        ImageDocument.saveOptionsStore = savedStore
        UserDefaults().removePersistentDomain(forName: suite)
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
        removeDefaultsSuite(suite)
    }

    // MARK: Fixtures and helpers

    /// A 160 x 120 opaque photo-like JPEG with EXIF (comment, camera, date),
    /// GPS, an XMP title and an embedded thumbnail, written by ImageIO.
    func cameraJPEG() throws -> URL {
        let (w, h) = (160, 120)
        var px = [UInt8](repeating: 255, count: w * h * 4)
        for y in 0..<h {
            for x in 0..<w {
                let i = (y * w + x) * 4
                let fx = Double(x) / Double(w), fy = Double(y) / Double(h)
                px[i] = UInt8(127.5 + 127 * sin(fx * 9 + fy * 3))
                px[i + 1] = UInt8(127.5 + 127 * cos(fy * 7 - fx * 2))
                px[i + 2] = UInt8(127.5 + 127 * sin((fx + fy) * 5))
            }
        }
        let url = dir.appendingPathComponent("camera.jpg")
        let image = try ImageIOExporter.cgImage(rgba: px, width: w, height: h, icc: srgbICC())
        let dest = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString, 1, nil))
        let props: [CFString: Any] = [
            kCGImageDestinationLossyCompressionQuality: 0.95,
            kCGImageDestinationEmbedThumbnail: true,
            kCGImagePropertyExifDictionary: [
                kCGImagePropertyExifUserComment: "M50 fixture",
                kCGImagePropertyExifDateTimeOriginal: "2026:10:24 12:34:56",
                kCGImagePropertyExifLensModel: "Test Lens 35mm",
            ],
            kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "OpenPainter", kCGImagePropertyTIFFModel: "M50"],
            kCGImagePropertyIPTCDictionary: [kCGImagePropertyIPTCCaptionAbstract: "M50 caption"],
            kCGImagePropertyGPSDictionary: [
                kCGImagePropertyGPSLatitude: Self.lat, kCGImagePropertyGPSLatitudeRef: "N",
                kCGImagePropertyGPSLongitude: Self.lon, kCGImagePropertyGPSLongitudeRef: "W",
            ],
        ]
        let md = try XCTUnwrap(CGImageMetadataCreateMutable())
        XCTAssertTrue(CGImageMetadataSetValueWithPath(md, nil, "dc:title" as CFString, "M50 title" as CFString))
        CGImageDestinationAddImageAndMetadata(dest, image, md, props as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        // The fixture itself carries GPS in EXIF and a thumbnail.
        let raw = try Data(contentsOf: url)
        XCTAssertGreaterThan(MetadataScan.tiffStructures(raw).gpsPointers, 0, "fixture has a GPS IFD")
        XCTAssertGreaterThan(MetadataScan.tiffStructures(raw).withIFD1, 0, "fixture has a thumbnail IFD")
        return url
    }

    func load(_ url: URL) throws -> LoadedImage {
        let l = try DocumentLoader.open(url, engine: fx.engine)
        opened.append(l.document)
        return l
    }

    func props(_ data: Data) throws -> [String: Any] {
        let s = try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil))
        return CGImageSourceCopyPropertiesAtIndex(s, 0, nil) as? [String: Any] ?? [:]
    }

    /// XMP tag paths ImageIO reads back.
    func xmpPaths(_ data: Data) throws -> [String] {
        let s = try XCTUnwrap(CGImageSourceCreateWithData(data as CFData, nil))
        guard let md = CGImageSourceCopyMetadataAtIndex(s, 0, nil) else { return [] }
        var out: [String] = []
        CGImageMetadataEnumerateTagsUsingBlock(md, nil, [kCGImageMetadataEnumerateRecursively: true] as CFDictionary) { p, _ in
            out.append(p as String)
            return true
        }
        return out
    }

    /// Decodes a saved file: PNG through the engine (exact bytes), others through ImageIO.
    func decode(_ data: Data, type: String) throws -> [UInt8] {
        if type == "public.png" {
            let url = dir.appendingPathComponent("decode-\(UUID().uuidString).png")
            try data.write(to: url)
            let d = try fx.engine.openPNG(path: url.path)
            defer { try? d.close() }
            return try d.pixels()
        }
        return try ImageIOImporter.decode(data: data).rgba
    }

    /// Peak signal-to-noise ratio over R, G and B in dB (peak 255).
    static func psnr(_ a: [UInt8], _ b: [UInt8]) -> Double {
        var sum = 0.0, n = 0.0
        for i in stride(from: 0, to: min(a.count, b.count), by: 4) {
            for c in 0..<3 {
                let d = Double(a[i + c]) - Double(b[i + c])
                sum += d * d
                n += 1
            }
        }
        let mse = sum / n
        return mse == 0 ? .infinity : 10 * log10(255 * 255 / mse)
    }

    func plasma() throws -> SavePixels {
        let d = try fx.engine.openPNG(path: fixtures.appendingPathComponent("plasma_alpha_512x384.png").path)
        opened.append(d)
        return try DocumentSaver.savePixels(d)
    }

    func encodable(_ types: [String]) -> [String] {
        types.filter { $0 == "public.png" || ImageFormats.imageIOSaveTypes().contains($0) }
    }

    // MARK: Lossless

    func testEveryLosslessSettingKeepsEveryPixelByte() throws {
        let px = try plasma()
        XCTAssertTrue(px.rgba.enumerated().contains { $0.offset % 4 == 3 && $0.element != 255 && $0.element != 0 },
                      "the fixture has partial alpha")
        var o = SaveOptions()
        var pngSizes: [Int] = []
        for level in 0...9 {
            o.pngLevel = level
            let data = try SaveEncoder.encode(px, type: "public.png", options: o, metadata: nil)
            pngSizes.append(data.count)
            XCTAssertTrue(try decode(data, type: "public.png") == px.rgba, "PNG level \(level) keeps every pixel byte")
        }
        XCTAssertLessThan(pngSizes[9], pngSizes[0], "level 9 is smaller than level 0: \(pngSizes)")
        var tiffSizes: [TIFFCompression: Int] = [:]
        for c in TIFFCompression.allCases {
            o.tiffCompression = c
            let data = try SaveEncoder.encode(px, type: "public.tiff", options: o, metadata: nil)
            tiffSizes[c] = data.count
            let tiff = try props(data)[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
            XCTAssertEqual((tiff?[kCGImagePropertyTIFFCompression as String] as? NSNumber)?.intValue, c.tag, "\(c)")
            XCTAssertTrue(try decode(data, type: "public.tiff") == px.rgba, "TIFF \(c) keeps every pixel byte")
        }
        XCTAssertLessThan(tiffSizes[.lzw]!, tiffSizes[.none]!)
        XCTAssertLessThan(tiffSizes[.deflate]!, tiffSizes[.none]!)
    }

    // MARK: Lossy

    /// JPEG, HEIC and AVIF on an opaque 512 x 384 image (the plasma fixture
    /// on white). PSNR floors over R, G and B (peak 255). The fixture is a
    /// smooth plasma, so every codec scores high; measured on macOS 27 the
    /// lowest of JPEG, HEIC and AVIF were 48.3, 46.6, 43.3, 38.8 and 32.9 dB at
    /// quality 95, 75, 50, 25 and 10. Each floor is 3 to 4 dB under that, so a
    /// codec update can move a little, while a wrong subsampling, a color
    /// channel swap or a quality ignored (all far lower or not falling) fails.
    /// Above 40 dB differences are invisible at 100%; 30 dB is the usual
    /// lower line for an acceptable 8-bit photo.
    func testLossySizeFallsWithQualityAndStaysAboveThePSNRFloor() throws {
        var px = try plasma()
        px.rgba = DocumentSaver.onWhite(px.rgba)
        let floors: [(Int, Double)] = [(95, 44), (75, 43), (50, 40), (25, 35), (10, 29)]
        for type in encodable(["public.jpeg", "public.heic", "public.avif"]) {
            var lastSize = Int.max
            for (q, floor) in floors {
                var o = SaveOptions()
                if type == "public.jpeg" { o.jpegQuality = q } else { o.quality = q }
                let data = try SaveEncoder.encode(px, type: type, options: o, metadata: nil)
                let p = Self.psnr(try decode(data, type: type), px.rgba)
                print("M50 \(type) quality \(q): \(data.count) bytes, PSNR \(String(format: "%.2f", p)) dB")
                XCTAssertLessThan(data.count, lastSize, "\(type): quality \(q) is smaller than the one above")
                XCTAssertGreaterThanOrEqual(p, floor, "\(type) quality \(q) PSNR")
                lastSize = data.count
            }
        }
        // Chroma subsampling: less color data, smaller file.
        var sizes: [ChromaSubsampling: Int] = [:]
        for s in ChromaSubsampling.allCases {
            var o = SaveOptions()
            o.jpegQuality = 90
            o.jpegSubsampling = s
            sizes[s] = try SaveEncoder.encode(px, type: "public.jpeg", options: o, metadata: nil).count
        }
        XCTAssertGreaterThan(sizes[.s444]!, sizes[.s422]!)
        XCTAssertGreaterThan(sizes[.s422]!, sizes[.s420]!)
    }

    func testGIFDitheringIsAnOption() throws {
        var px = try plasma()
        px.rgba = DocumentSaver.onWhite(px.rgba)
        var o = SaveOptions()
        o.gifDither = false
        let plain = try SaveEncoder.encode(px, type: "com.compuserve.gif", options: o, metadata: nil)
        o.gifDither = true
        let dithered = try SaveEncoder.encode(px, type: "com.compuserve.gif", options: o, metadata: nil)
        XCTAssertNotEqual(plain, dithered)
        XCTAssertEqual(try ImageIOImporter.decode(data: dithered).width, 512)
    }

    // MARK: Metadata

    let metadataTypes = ["public.jpeg", "public.png", "public.tiff", "public.heic", "public.avif"]

    func testKeepAllKeepsTheFileEXIFFieldsAndLocation() throws {
        let loaded = try load(try cameraJPEG())
        XCTAssertNotNil(loaded.metadata.gps)
        XCTAssertNotNil(loaded.metadata.properties[kCGImagePropertyIPTCDictionary as String], "fixture has IPTC")
        let px = try DocumentSaver.savePixels(loaded.document)
        for type in encodable(metadataTypes) {
            let data = try SaveEncoder.encode(px, type: type, options: SaveOptions(), metadata: loaded.metadata)
            let p = try props(data)
            let exif = p[kCGImagePropertyExifDictionary as String] as? [String: Any]
            let tiff = p[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
            XCTAssertEqual(exif?[kCGImagePropertyExifUserComment as String] as? String, "M50 fixture", type)
            XCTAssertEqual(exif?[kCGImagePropertyExifDateTimeOriginal as String] as? String, "2026:10:24 12:34:56", type)
            XCTAssertEqual(exif?[kCGImagePropertyExifLensModel as String] as? String, "Test Lens 35mm", type)
            XCTAssertEqual(tiff?[kCGImagePropertyTIFFMake as String] as? String, "OpenPainter", type)
            XCTAssertEqual(tiff?[kCGImagePropertyTIFFModel as String] as? String, "M50", type)
            let gps = p[kCGImagePropertyGPSDictionary as String] as? [String: Any]
            XCTAssertEqual((gps?[kCGImagePropertyGPSLatitude as String] as? NSNumber)?.doubleValue ?? 0, Self.lat,
                           accuracy: 0.001, type)
            XCTAssertTrue(try xmpPaths(data).contains("dc:title"), "\(type) keeps the XMP title")
            // IPTC: as IPTC for ImageIO formats, as XMP (dc:description) for the engine's PNG and JPEG.
            XCTAssertNotNil(data.range(of: Data("M50 caption".utf8)), "\(type) keeps the IPTC caption")
            // The positive control for the strip test's raw scan.
            XCTAssertGreaterThan(MetadataScan.tiffStructures(data).gpsPointers, 0, "\(type) keeps the GPS IFD")
        }
    }

    func testStripLocationRemovesGPSFromEXIFXMPAndThumbnails() throws {
        let loaded = try load(try cameraJPEG())
        XCTAssertTrue(String(data: try XCTUnwrap(loaded.metadata.xmp), encoding: .utf8)?.contains("GPS") == true,
                      "the imported XMP holds exif:GPS tags")
        let px = try DocumentSaver.savePixels(loaded.document)
        var o = SaveOptions()
        o.metadata = .stripLocation
        for type in encodable(metadataTypes) {
            let data = try SaveEncoder.encode(px, type: type, options: o, metadata: loaded.metadata)
            let p = try props(data)
            XCTAssertNil(p[kCGImagePropertyGPSDictionary as String], "\(type): no GPS in EXIF")
            let paths = try xmpPaths(data)
            XCTAssertFalse(paths.contains { $0.contains("GPS") }, "\(type): no GPS in XMP: \(paths)")
            let scan = MetadataScan.tiffStructures(data)
            XCTAssertEqual(scan.gpsPointers, 0, "\(type): no GPS IFD in any TIFF structure")
            XCTAssertEqual(scan.withIFD1, 0, "\(type): no embedded thumbnail")
            XCTAssertEqual(MetadataScan.gpsNames(data), [], "\(type): no GPS names in the bytes")
            XCTAssertNil(p[kCGImagePropertyThumbnailImages as String], type)
            // Only location goes.
            let exif = p[kCGImagePropertyExifDictionary as String] as? [String: Any]
            XCTAssertEqual(exif?[kCGImagePropertyExifUserComment as String] as? String, "M50 fixture", type)
            XCTAssertTrue(paths.contains("dc:title"), "\(type) keeps the XMP title")
        }
    }

    func testStripAllRemovesEXIFXMPGPSAndIPTC() throws {
        let loaded = try load(try cameraJPEG())
        let px = try DocumentSaver.savePixels(loaded.document)
        var o = SaveOptions()
        o.metadata = .stripAll
        for type in encodable(metadataTypes) {
            let data = try SaveEncoder.encode(px, type: type, options: o, metadata: loaded.metadata)
            let p = try props(data)
            let exif = p[kCGImagePropertyExifDictionary as String] as? [String: Any]
            XCTAssertNil(exif?[kCGImagePropertyExifUserComment as String], type)
            XCTAssertNil(exif?[kCGImagePropertyExifLensModel as String], type)
            XCTAssertNil((p[kCGImagePropertyTIFFDictionary as String] as? [String: Any])?[kCGImagePropertyTIFFMake as String], type)
            XCTAssertNil(p[kCGImagePropertyGPSDictionary as String], type)
            XCTAssertNil(p[kCGImagePropertyIPTCDictionary as String], type)
            XCTAssertNil(data.range(of: Data("M50 caption".utf8)), "\(type): no IPTC caption anywhere")
            let paths = try xmpPaths(data)
            XCTAssertFalse(paths.contains("dc:title"), "\(type): \(paths)")
            XCTAssertFalse(paths.contains { $0.contains("GPS") }, type)
            XCTAssertEqual(MetadataScan.tiffStructures(data).gpsPointers, 0, type)
            XCTAssertEqual(MetadataScan.gpsNames(data), [], type)
        }
    }

    func testPNGMetadataRoundTripsThroughTheEngineReaderToo() throws {
        let loaded = try load(try cameraJPEG())
        let px = try DocumentSaver.savePixels(loaded.document)
        let data = try SaveEncoder.encode(px, type: "public.png", options: SaveOptions(), metadata: loaded.metadata)
        let types = PNGChunks.all(data).map(\.type)
        XCTAssertTrue(types.contains("eXIf"), "\(types)")
        XCTAssertTrue(types.contains("iTXt"), "\(types)")
        XCTAssertTrue(types.contains("pHYs"), "\(types)")
        // Reopening the PNG (engine pixels, ImageIO metadata) keeps the EXIF.
        let url = dir.appendingPathComponent("kept.png")
        try data.write(to: url)
        let again = try load(url)
        XCTAssertEqual(again.metadata.exif?[kCGImagePropertyExifUserComment as String] as? String, "M50 fixture")
        XCTAssertTrue(try again.document.pixels() == px.rgba, "pixels exact")
    }

    // MARK: Resolution

    func testResolutionIsWrittenAndAReopenedFileKeepsIt() throws {
        let doc = try fx.engine.newDocument(width: 40, height: 30, dpi: 300, background: [200, 100, 50, 255])
        opened.append(doc)
        let px = try DocumentSaver.savePixels(doc)
        XCTAssertEqual(px.dpi, 300)
        for type in encodable(["public.png", "public.jpeg", "public.tiff", "public.heic", "public.avif"]) {
            let data = try SaveEncoder.encode(px, type: type, options: SaveOptions(), metadata: nil)
            let p = try props(data)
            XCTAssertEqual((p[kCGImagePropertyDPIWidth as String] as? NSNumber)?.doubleValue ?? 0, 300, accuracy: 0.01, type)
            let url = dir.appendingPathComponent("dpi.\(ImageFormats.fileExtension(type) ?? "img")")
            try data.write(to: url)
            let back = try load(url)
            XCTAssertEqual(try DocumentSaver.resolution(back.document), 300, accuracy: 0.01, "\(type) reopens at 300 dpi")
        }
    }

    // MARK: Persistence

    func testOptionsPersistPerFormat() throws {
        let store = ImageDocument.saveOptionsStore
        XCTAssertNil(store.load("public.jpeg"))
        XCTAssertEqual(store.options("public.jpeg"), SaveOptions(), "defaults first")
        var jpeg = SaveOptions()
        jpeg.jpegQuality = 42
        jpeg.jpegSubsampling = .s444
        jpeg.metadata = .stripLocation
        var png = SaveOptions()
        png.pngLevel = 9
        png.metadata = .stripAll
        store.save(jpeg, for: "public.jpeg")
        store.save(png, for: "public.png")
        let again = SaveOptionsStore(defaults: UserDefaults(suiteName: suite)!)
        XCTAssertEqual(again.load("public.jpeg"), jpeg)
        XCTAssertEqual(again.load("public.png"), png)
        XCTAssertNil(again.load("public.tiff"), "each format keeps its own")
        var avif = SaveOptions()
        avif.quality = 100
        XCTAssertEqual(avif.clamped(for: "public.avif").quality, 99, "AVIF tops out at 99")
    }

    // MARK: Size preview

    func testSizePreviewCancelsStaleWorkAndShowsOnlyTheLastResult() throws {
        let gate = DispatchSemaphore(value: 0)
        let calls = Counter()
        let preview = SaveSizePreview(decodeImage: false) { o in
            if calls.next() == 1 { gate.wait() }  // the first encode runs long
            return Data(count: o.jpegQuality * 10)
        }
        var shown: [SaveSizePreview.Result] = []
        preview.onResult = { shown.append($0) }
        var o = SaveOptions()
        o.jpegQuality = 1
        preview.request(o)
        XCTAssertTrue(waitUntil(5) { preview.encodesStarted == 1 }, "the first encode started")
        for q in 2...5 {
            o.jpegQuality = q
            preview.request(o)
        }
        gate.signal()
        XCTAssertTrue(waitUntil(5) { !shown.isEmpty })
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        XCTAssertEqual(shown.map(\.generation), [5], "only the last request's result shows")
        XCTAssertEqual(shown.first?.bytes, 50)
        XCTAssertEqual(preview.encodesStarted, 2, "requests 2 to 4 were replaced before they started")
        // After cancel nothing more shows.
        o.jpegQuality = 9
        preview.request(o)
        preview.cancel()
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        XCTAssertEqual(shown.count, 1)
    }

    final class Counter: @unchecked Sendable {
        private let lock = NSLock()
        private var n = 0
        func next() -> Int { lock.withLock { n += 1; return n } }
    }

    // MARK: The sheet

    func testSheetShowsTheSizeAndPreviewAndUpdatesOnChange() throws {
        let px = try plasma()
        let sheet = SaveOptionsController(type: "public.jpeg", options: SaveOptions(), pixels: px, metadata: nil)
        XCTAssertTrue(waitUntil(10) { sheet.shownBytes != nil }, "the first size arrives")
        let high = try XCTUnwrap(sheet.shownBytes)
        XCTAssertTrue(sheet.sizeLabel.stringValue.hasPrefix("File size: "), sheet.sizeLabel.stringValue)
        XCTAssertNotNil(sheet.previewView.image, "lossy formats show the encoded image")
        sheet.update { $0.jpegQuality = 10 }
        XCTAssertEqual(sheet.sizeLabel.stringValue, "Measuring…")
        XCTAssertTrue(waitUntil(10) { (sheet.shownBytes ?? .max) < high }, "lower quality, smaller file")
        XCTAssertEqual(sheet.qualitySlider.integerValue, 10)
        var result: SaveOptions??
        sheet.onClose = { result = .some($0) }
        sheet.ok(nil)
        XCTAssertEqual(result??.jpegQuality, 10)
        let png = SaveOptionsController(type: "public.png", options: SaveOptions(), pixels: px, metadata: nil)
        XCTAssertNil(png.previewView.superview, "lossless PNG shows no preview image")
        var cancelled: SaveOptions?? = .none
        png.onClose = { cancelled = .some($0) }
        png.cancel(nil)
        XCTAssertEqual(cancelled.map { $0 == nil }, true)
    }

    func open(_ url: URL, type: String) throws -> ImageDocument {
        let doc = try ImageDocument(contentsOf: url, ofType: type)
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        return doc
    }

    /// The save panel's and File > Save's path (with a delegate).
    func uiSave(_ doc: ImageDocument, to url: URL, type: String, _ op: NSDocument.SaveOperationType) -> Bool {
        let recorder = SaveRecorder()
        doc.save(to: url, ofType: type, for: op, delegate: recorder,
                 didSave: #selector(SaveRecorder.document(_:didSave:contextInfo:)), contextInfo: nil)
        XCTAssertTrue(waitUntil(20) { recorder.result != nil }, "save finished")
        return recorder.result ?? false
    }

    func testSaveAsSaveAndSaveACopyAskForOptionsAndUseThem() async throws {
        let doc = try open(try cameraJPEG(), type: "public.jpeg")
        let jpg = dir.appendingPathComponent("as.jpg")
        var chosen = SaveOptions()
        chosen.jpegQuality = 30
        chosen.metadata = .stripLocation
        promptAnswers = [chosen]
        XCTAssertTrue(uiSave(doc, to: jpg, type: "public.jpeg", .saveAsOperation))
        XCTAssertEqual(prompts.map(\.type), ["public.jpeg"], "Save As asks")
        XCTAssertEqual(prompts.first?.initial, SaveOptions(), "first time: the defaults")
        let first = try Data(contentsOf: jpg)
        XCTAssertNil(try props(first)[kCGImagePropertyGPSDictionary as String], "the sheet's strip choice was used")
        XCTAssertEqual(ImageDocument.saveOptionsStore.load("public.jpeg"), chosen, "remembered for JPEG")
        // Save again in the same format: no sheet, same options.
        XCTAssertTrue(uiSave(doc, to: jpg, type: "public.jpeg", .saveOperation))
        XCTAssertEqual(prompts.count, 1, "Save after Save As does not ask again")
        XCTAssertEqual(try Data(contentsOf: jpg).count, first.count, "same options, same size")
        // Save a Copy always asks; Cancel writes nothing.
        let copy = dir.appendingPathComponent("copy.png")
        promptAnswers = [nil]
        XCTAssertFalse(uiSave(doc, to: copy, type: "public.png", .saveToOperation))
        XCTAssertEqual(prompts.last?.type, "public.png")
        XCTAssertFalse(FileManager.default.fileExists(atPath: copy.path), "Cancel saves nothing")
        // A second image starts from the remembered JPEG options.
        let other = try open(try pngCopy(), type: "public.png")
        promptAnswers = []
        XCTAssertTrue(uiSave(other, to: dir.appendingPathComponent("other.jpg"), type: "public.jpeg", .saveAsOperation))
        XCTAssertEqual(prompts.last?.initial, chosen, "the last JPEG options are the starting point")
        // A direct save (no menu or panel) never shows the sheet.
        let before = prompts.count
        var done = false
        other.save(to: dir.appendingPathComponent("direct.tiff"), ofType: "public.tiff", for: .saveToOperation) { _ in done = true }
        XCTAssertTrue(waitUntil(20) { done })
        XCTAssertEqual(prompts.count, before)
    }

    func testSaveOfALossyFormatNeverChosenAsksOnce() throws {
        let jpg = dir.appendingPathComponent("photo.jpg")
        try FileManager.default.copyItem(at: try cameraJPEG(), to: jpg)
        let doc = try open(jpg, type: "public.jpeg")
        XCTAssertTrue(uiSave(doc, to: jpg, type: "public.jpeg", .saveOperation))
        XCTAssertEqual(prompts.map(\.type), ["public.jpeg"], "JPEG options never chosen for this image: ask")
        XCTAssertTrue(uiSave(doc, to: jpg, type: "public.jpeg", .saveOperation))
        XCTAssertEqual(prompts.count, 1, "chosen once: Save does not ask again")
        let native = dir.appendingPathComponent("n.opaint")
        XCTAssertTrue(uiSave(doc, to: native, type: ImageFormats.opaintIdentifier, .saveAsOperation))
        XCTAssertEqual(prompts.count, 1, ".opaint has no options")
        // Save to a lossless format uses the last options without asking.
        let url = try pngCopy()
        let png = try open(url, type: "public.png")
        XCTAssertTrue(uiSave(png, to: url, type: "public.png", .saveOperation))
        XCTAssertEqual(prompts.count, 1, "PNG Save does not ask")
    }

    func pngCopy() throws -> URL {
        let url = dir.appendingPathComponent("plasma-\(UUID().uuidString).png")
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        return url
    }

    // MARK: Visual check

    /// Renders the sheet for JPEG, PNG, TIFF and GIF in light and dark when
    /// M50_PNG_DIR is set.
    func testRenderSaveOptionsSheet() throws {
        guard let out = ProcessInfo.processInfo.environment["M50_PNG_DIR"] else {
            throw XCTSkip("set M50_PNG_DIR to render the save options sheet")
        }
        let folder = URL(fileURLWithPath: out)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let loaded = try load(try cameraJPEG())
        let px = try DocumentSaver.savePixels(loaded.document)
        for type in ["public.jpeg", "public.png", "public.tiff", "com.compuserve.gif", "public.heic"] {
            for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
                let sheet = SaveOptionsController(type: type, options: SaveOptions(), pixels: px,
                                                  metadata: type == "public.tiff" ? nil : loaded.metadata)
                let view = try XCTUnwrap(sheet.window?.contentView)
                view.appearance = NSAppearance(named: appearance)
                sheet.window?.appearance = NSAppearance(named: appearance)
                XCTAssertTrue(waitUntil(10) { sheet.shownBytes != nil })
                view.layoutSubtreeIfNeeded()
                let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
                NSAppearance(named: appearance)!.performAsCurrentDrawingAppearance {
                    view.cacheDisplay(in: view.bounds, to: rep)
                }
                let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
                let short = ImageFormats.shortName(type).lowercased()
                try png.write(to: folder.appendingPathComponent("save-options-\(short)-\(name).png"))
                sheet.cancel(nil)
            }
        }
    }
}

/// Records `document:didSave:contextInfo:`.
final class SaveRecorder: NSObject {
    var result: Bool?
    @objc func document(_ doc: NSDocument, didSave: Bool, contextInfo: UnsafeMutableRawPointer?) {
        result = didSave
    }
}
