// Release review fix round 4, the Mac side. Item 1: everyday JPEGs keep
// plain Save (only GIF, TIFF, PNG and WebP count frames). Item 2: a Magic
// Wand that the 16 GiB total refuses leaves no session and keeps the limit
// text. Item 3: Don't Save after a timed-out wait closes at once and never
// blocks the main thread or other images. Item 4: Quit waits once per image.
// Item 5: a recovered image that needs Save As says why.
import AppKit
import Compression
import ImageIO
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class FixRound4Tests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var root: URL!
    var docs: [ImageDocument] = []
    var unsavedAnswers: [UnsavedChoice] = []
    var unsavedAsked = 0
    var askedAt: [Date] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedUnsaved: ImageDocument.UnsavedPrompt!
    var savedSaveOptions: ImageDocument.SaveOptionsPrompt!

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        root = dir.appendingPathComponent("Recovery", isDirectory: true)
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedUnsaved = ImageDocument.unsavedPrompt
        savedSaveOptions = ImageDocument.saveOptionsPrompt
        // The JPEG options sheet answers with the options it was given.
        ImageDocument.saveOptionsPrompt = { _, _, options, done in done(options) }
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        ImageDocument.unsavedPrompt = { [unowned self] _, done in
            unsavedAsked += 1
            askedAt.append(Date())
            done(unsavedAnswers.isEmpty ? .cancel : unsavedAnswers.removeFirst())
        }
        RecoveryCenter.root = root
        RecoveryCenter.timing = RecoveryTiming(settle: .milliseconds(50), maxInterval: .seconds(5))
    }

    override func tearDown() async throws {
        for d in docs {
            for wc in d.windowControllers {
                if let w = wc.window, let sheet = w.attachedSheet { w.endSheet(sheet, returnCode: .cancel) }
                wc.window?.orderOut(nil)
            }
            d.close()
        }
        docs.removeAll()
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        ImageDocument.unsavedPrompt = savedUnsaved
        ImageDocument.saveOptionsPrompt = savedSaveOptions
        ImageDocument.settleTimeout = .seconds(600)
        RecoveryCenter.root = nil
        RecoveryCenter.timing = RecoveryTiming()
        fx.tearDown()
        try? FileManager.default.removeItem(at: dir)
    }

    // MARK: Helpers

    func pngCopy(_ name: String = "plasma.png") throws -> URL {
        let url = dir.appendingPathComponent(name)
        try FileManager.default.copyItem(at: fixtures.appendingPathComponent("plasma_alpha_512x384.png"), to: url)
        return url
    }

    @discardableResult
    func open(_ url: URL, type: String? = nil) throws -> ImageDocument {
        let t = type ?? UTType(filenameExtension: url.pathExtension)?.identifier ?? "public.data"
        let doc = try ImageDocument(contentsOf: url, ofType: t)
        show(doc)
        return doc
    }

    func show(_ doc: ImageDocument) {
        docs.append(doc)
        if doc.windowControllers.isEmpty { doc.makeWindowControllers() }
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
    }

    func wc(_ doc: ImageDocument) throws -> EditorWindowController {
        try XCTUnwrap(doc.windowControllers.first as? EditorWindowController)
    }

    func eventually(_ seconds: Double = 10, _ condition: @escaping () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    func invert(_ doc: ImageDocument) async throws {
        let status = try await wc(doc).editor.runAndWait("image.invert_colors")
        XCTAssertEqual(status, "ok")
    }

    /// Queues `count` slow inverts of about 4 s each on a 512 x 384 image
    /// (1 s per tile, 4 tiles).
    func queueSlowWork(_ doc: ImageDocument, count: Int = 1, delayMS: Int = 1000) throws {
        let e = try XCTUnwrap(doc.engineDocument)
        for _ in 0..<count { _ = try e.dispatch("test.slow_invert", ["delay_ms": delayMS]) }
    }

    func quit(_ docs: [ImageDocument]) -> ReviewRecorder {
        let recorder = ReviewRecorder()
        ImageDocumentController.reviewUnsaved(docs, controller: NSDocumentController.shared, delegate: recorder,
                                              didReviewAllSelector: #selector(ReviewRecorder.documentController(_:didReviewAll:contextInfo:)),
                                              contextInfo: nil)
        return recorder
    }

    /// Records the longest time the main run loop went without running a
    /// 10 ms timer.
    final class MainThreadWatch {
        private var timer: Timer?
        private var last = Date()
        private(set) var longestGap: TimeInterval = 0

        @MainActor
        func start() {
            last = Date()
            let t = Timer(timeInterval: 0.01, repeats: true) { [weak self] _ in
                guard let self else { return }
                let now = Date()
                self.longestGap = max(self.longestGap, now.timeIntervalSince(self.last))
                self.last = now
            }
            RunLoop.main.add(t, forMode: .common)
            timer = t
        }

        @MainActor
        func stop() {
            timer?.invalidate()
            longestGap = max(longestGap, Date().timeIntervalSince(last))
        }
    }

    func cgImage(width: Int = 16, height: Int = 8, value: UInt8 = 0) throws -> CGImage {
        var px = [UInt8](repeating: 0, count: width * height * 4)
        for i in 0..<(width * height) {
            px[i * 4] = UInt8(truncatingIfNeeded: i * 7) &+ value
            px[i * 4 + 1] = 120
            px[i * 4 + 2] = UInt8(truncatingIfNeeded: 255 - i)
            px[i * 4 + 3] = 255
        }
        return try XCTUnwrap(CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: CGDataProvider(data: Data(px) as CFData)!, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent))
    }

    func encode(_ image: CGImage, _ type: String, frames: Int = 1) throws -> Data {
        let out = NSMutableData()
        let dest = try XCTUnwrap(CGImageDestinationCreateWithData(out, type as CFString, frames, nil))
        for _ in 0..<frames { CGImageDestinationAddImage(dest, image, nil) }
        XCTAssertTrue(CGImageDestinationFinalize(dest))
        return out as Data
    }

    func writeImageIO(_ type: String, _ name: String, frames: Int = 1) throws -> URL {
        let url = dir.appendingPathComponent(name)
        try encode(try cgImage(), type, frames: frames).write(to: url)
        return url
    }

    /// A camera-style JPEG: a primary image with an MPF (CIPA DC-007)
    /// segment that points at a second JPEG appended after it, as cameras
    /// store a preview, a depth image or a stereo pair (the entries here are
    /// the stereo kind, which ImageIO counts when the name says MPO). ImageIO cannot write one
    /// (a JPEG destination takes one image), so it is built here.
    func mpfJPEG() throws -> Data {
        let primary = try encode(try cgImage(width: 32, height: 16), "public.jpeg")
        let second = try encode(try cgImage(width: 16, height: 8, value: 90), "public.jpeg")
        func be32(_ v: UInt32) -> [UInt8] { [UInt8(v >> 24), UInt8((v >> 16) & 255), UInt8((v >> 8) & 255), UInt8(v & 255)] }
        func be16(_ v: UInt16) -> [UInt8] { [UInt8(v >> 8), UInt8(v & 255)] }
        // MP header (big-endian TIFF layout) and the MP index IFD: version,
        // number of images, and the two 16-byte MP entries after the IFD.
        var mp: [UInt8] = Array("MM".utf8) + [0, 42] + be32(8)
        mp += be16(3)
        mp += be16(0xB000) + be16(7) + be32(4) + Array("0100".utf8)
        mp += be16(0xB001) + be16(4) + be32(1) + be32(2)
        mp += be16(0xB002) + be16(7) + be32(32) + be32(UInt32(8 + 2 + 3 * 12 + 4))
        mp += be32(0)
        let segmentLength = 2 + 4 + mp.count + 32
        let firstSize = primary.count + 2 + segmentLength
        let mpStart = 2 + 4 + 4  // SOI, the APP2 marker and length, "MPF\0"
        var entries = be32(0x2002_0002) + be32(UInt32(firstSize)) + be32(0) + be16(0) + be16(0)
        entries += be32(0x0002_0002) + be32(UInt32(second.count)) + be32(UInt32(firstSize - mpStart)) + be16(0) + be16(0)
        let segment: [UInt8] = [0xFF, 0xE2] + be16(UInt16(segmentLength)) + Array("MPF\0".utf8) + mp + entries
        var file = Data(primary.prefix(2))
        file.append(contentsOf: segment)
        file.append(primary.dropFirst(2))
        file.append(second)
        return file
    }

    func imageIOPixels(_ url: URL) throws -> Data {
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
        return try XCTUnwrap(image.dataProvider?.data as Data?)
    }

    // MARK: Item 1: everyday JPEGs keep plain Save

    /// ImageIO reads the camera JPEG's MPF as a second image when the name
    /// says MPO; the content is still a JPEG, and its extra image is a
    /// preview or depth map, not a frame Save would drop.
    func testAJPEGThatImageIOCountsAsTwoImagesNeedsNoSaveAs() throws {
        let url = dir.appendingPathComponent("camera.mpo")
        try mpfJPEG().write(to: url)
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
        XCTAssertEqual(CGImageSourceGetCount(source), 2, "ImageIO counts two images in this JPEG")
        let reasons = ImageFormats.plainSaveReasons(url, contentType: "public.jpeg")
        XCTAssertFalse(reasons.contains { $0.contains("frames") }, "\(reasons)")
    }

    func testACameraJPEGWithAnMPFImageSavesInPlace() async throws {
        let url = dir.appendingPathComponent("camera.jpg")
        try mpfJPEG().write(to: url)
        let before = try imageIOPixels(url)
        let doc = try open(url)
        XCTAssertFalse(doc.saveNeedsNewFile)
        XCTAssertTrue(doc.plainSaveAllowed, "a camera JPEG keeps plain Save")
        try await invert(doc)
        let w = try XCTUnwrap(try wc(doc).window)
        doc.save(nil)
        let saved = await eventually(20) { !doc.isDocumentEdited }
        XCTAssertTrue(saved, "Save wrote the file")
        XCTAssertFalse(w.attachedSheet is NSSavePanel, "Save did not ask where")
        XCTAssertNotEqual(try imageIOPixels(url), before, "the file holds the inverted image")
    }

    func testFramesStillCountForGIFTIFFAndAnimatedPNG() throws {
        let gif = try writeImageIO("com.compuserve.gif", "anim.gif", frames: 3)
        XCTAssertTrue(ImageFormats.plainSaveReasons(gif, contentType: "com.compuserve.gif").contains { $0.contains("3 frames") })
        let tiff = try writeImageIO("public.tiff", "pages.tiff", frames: 2)
        XCTAssertTrue(ImageFormats.plainSaveReasons(tiff, contentType: "public.tiff").contains { $0.contains("2 frames") })
        let apng = try writeImageIO("public.png", "anim.png", frames: 2)
        let source = try XCTUnwrap(CGImageSourceCreateWithURL(apng as CFURL, nil))
        if CGImageSourceGetCount(source) == 2 {
            XCTAssertTrue(ImageFormats.plainSaveReasons(apng, contentType: "public.png").contains { $0.contains("2 frames") })
        } else {
            print("APNG_NOT_WRITTEN: ImageIO wrote \(CGImageSourceGetCount(source)) frame(s)")
        }
    }

    // MARK: Item 2: a refused Magic Wand

    /// CRC-32 (ISO 3309), as PNG chunks use.
    static func crc32(_ bytes: [UInt8]) -> UInt32 {
        var c: UInt32 = 0xFFFF_FFFF
        for b in bytes {
            c ^= UInt32(b)
            for _ in 0..<8 { c = (c & 1) != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        }
        return c ^ 0xFFFF_FFFF
    }

    /// A transparent RGBA PNG `side` pixels square, deflated as a stream so
    /// the 1 GiB of zero rows is never held in memory.
    func transparentPNG(side: Int) throws -> Data {
        let total = (1 + side * 4) * side
        var deflated = [UInt8]()
        let chunk = [UInt8](repeating: 0, count: 1 << 20)
        let outSize = 1 << 16
        let out = UnsafeMutablePointer<UInt8>.allocate(capacity: outSize)
        defer { out.deallocate() }
        let stream = UnsafeMutablePointer<compression_stream>.allocate(capacity: 1)
        defer { stream.deallocate() }
        XCTAssertEqual(compression_stream_init(stream, COMPRESSION_STREAM_ENCODE, COMPRESSION_ZLIB), COMPRESSION_STATUS_OK)
        defer { compression_stream_destroy(stream) }
        var left = total
        try chunk.withUnsafeBufferPointer { src in
            var finished = false
            while !finished {
                let n = min(left, chunk.count)
                left -= n
                let last = left == 0
                stream.pointee.src_ptr = src.baseAddress!
                stream.pointee.src_size = n
                while true {
                    stream.pointee.dst_ptr = out
                    stream.pointee.dst_size = outSize
                    let status = compression_stream_process(stream, last ? Int32(COMPRESSION_STREAM_FINALIZE.rawValue) : 0)
                    guard status != COMPRESSION_STATUS_ERROR else { throw NSError(domain: "deflate", code: 1) }
                    deflated.append(contentsOf: UnsafeBufferPointer(start: out, count: outSize - stream.pointee.dst_size))
                    if status == COMPRESSION_STATUS_END {
                        finished = true
                        break
                    }
                    if !last, stream.pointee.src_size == 0, stream.pointee.dst_size > 0 { break }
                }
            }
        }
        // zlib wrapper: header, raw deflate, Adler-32 of all-zero bytes.
        let adler = (UInt32(total % 65521) << 16) | 1
        var zlib: [UInt8] = [0x78, 0x01] + deflated
        zlib += [UInt8(adler >> 24), UInt8((adler >> 16) & 255), UInt8((adler >> 8) & 255), UInt8(adler & 255)]
        func chunkBytes(_ type: String, _ body: [UInt8]) -> [UInt8] {
            let typed = Array(type.utf8) + body
            let n = UInt32(body.count), c = Self.crc32(typed)
            return [UInt8(n >> 24), UInt8((n >> 16) & 255), UInt8((n >> 8) & 255), UInt8(n & 255)] + typed
                + [UInt8(c >> 24), UInt8((c >> 16) & 255), UInt8((c >> 8) & 255), UInt8(c & 255)]
        }
        let s = UInt32(side)
        let side4: [UInt8] = [UInt8(s >> 24), UInt8((s >> 16) & 255), UInt8((s >> 8) & 255), UInt8(s & 255)]
        var png: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        png += chunkBytes("IHDR", side4 + side4 + [8, 6, 0, 0, 0])
        png += chunkBytes("IDAT", zlib)
        png += chunkBytes("IEND", [])
        return Data(png)
    }

    /// An .opaint file of 16 transparent 16384 x 16384 layers: the whole
    /// 16 GiB total, as a file may hold when it stores no selection mask.
    /// Every layer reads the same PNG entry.
    func fullOpaint() throws -> URL {
        let side = 16384, layers = 16
        let work = dir.appendingPathComponent("full", isDirectory: true)
        try FileManager.default.createDirectory(at: work.appendingPathComponent("data"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: work.appendingPathComponent("Thumbnails"), withIntermediateDirectories: true)
        try Data("image/openraster".utf8).write(to: work.appendingPathComponent("mimetype"))
        try transparentPNG(side: side).write(to: work.appendingPathComponent("data/layer.png"))
        let small = try Data(contentsOf: fixtures.appendingPathComponent("plasma_alpha_512x384.png"))
        try small.write(to: work.appendingPathComponent("mergedimage.png"))
        try small.write(to: work.appendingPathComponent("Thumbnails/thumbnail.png"))
        var xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><image version=\"0.0.6\" w=\"\(side)\" h=\"\(side)\" "
            + "xres=\"72\" yres=\"72\" xmlns:op=\"https://openpainter.dev/ns/1\" op:version=\"1.0\" op:dpi=\"72\" "
            + "op:active-layer=\"1\" op:selection=\"none\"><stack>"
        for i in 0..<layers {
            xml += "<layer src=\"data/layer.png\" name=\"L\(i)\" visibility=\"visible\" opacity=\"1\" x=\"0\" y=\"0\" "
                + "composite-op=\"svg:src-over\" op:id=\"\(i + 1)\"/>"
        }
        xml += "</stack></image>"
        try Data(xml.utf8).write(to: work.appendingPathComponent("stack.xml"))
        let url = dir.appendingPathComponent("full.opaint")
        func zip(_ args: [String]) throws {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
            p.currentDirectoryURL = work
            p.arguments = ["-q", "-X", "-0", url.path] + args
            try p.run()
            p.waitUntilExit()
            XCTAssertEqual(p.terminationStatus, 0, "zip \(args)")
        }
        try zip(["mimetype"])
        try zip(["-r", "stack.xml", "mergedimage.png", "Thumbnails", "data"])
        return url
    }

    func testAMagicWandRefusedAtThe16GiBTotalLeavesNoSessionAndKeepsTheLimitText() async throws {
        let url = try fullOpaint()
        let doc = try fx.engine.openOpaint(path: url.path)
        let info = try doc.info()
        XCTAssertEqual((info["layers"] as? [Any])?.count, 16)
        let editor = Editor(document: doc, pixelSize: CGSize(width: 16384, height: 16384), name: "full")
        fx.workspace.add(editor)
        let c = EditorWindowController(editor: editor, workspace: fx.workspace)
        c.window?.isReleasedWhenClosed = false
        fx.controllers.append(c)
        c.present()
        fx.workspace.tools.tool = .magicWand
        let text = "16 GiB"
        func check(_ why: String, limitShown: Bool = true, line: UInt = #line) async {
            let idle = await eventually { c.flood.isIdle }
            XCTAssertTrue(idle, why, line: line)
            // Results arrive on main-actor tasks; give late ones time.
            try? await Task.sleep(for: .milliseconds(300))
            let message = c.editor.message ?? ""
            if limitShown {
                XCTAssertTrue(message.contains(text), "\(why): the status bar keeps the limit: \(message)", line: line)
            }
            for bad in ["bad_argument", "failed", "internal"] {
                XCTAssertFalse(message.contains(bad), "\(why): \(message)", line: line)
            }
            XCTAssertNil(c.flood.session, "\(why): no session is left", line: line)
            XCTAssertFalse(c.optionsBar.floodSessionOpen, "\(why): no Finish control", line: line)
            XCTAssertNil(c.flood.handle.center, "\(why): no seed handle", line: line)
            XCTAssertEqual((try? doc.info())?["session_open"] as? Bool, false, line: line)
        }
        c.flood.click(at: CGPoint(x: 10, y: 10), secondary: false)
        await check("first click")
        // Enter (Finish) after the refusal has nothing to finish.
        XCTAssertFalse(c.flood.finish())
        await check("Finish after the refusal")
        // A second click is refused the same way.
        c.flood.click(at: CGPoint(x: 40, y: 40), secondary: false)
        await check("second click")
        // Finish pressed before the refusal arrives sends nothing that fails later.
        c.flood.click(at: CGPoint(x: 60, y: 60), secondary: false)
        c.flood.finish()
        await check("Finish before the refusal arrived")
        // Another tool, then the wand again: nothing fails later (a tool
        // change clears the status bar).
        fx.workspace.tools.tool = .paintBucket
        fx.workspace.tools.tool = .magicWand
        await check("tool change", limitShown: false)
        XCTAssertEqual(c.editor.info.historyLabels, ["Open"])
    }

    // MARK: Item 3: Don't Save after a timed-out wait

    func testDontSaveAfterATimedOutWaitClosesAtOnceWithoutBlockingAndQuitFinishes() async throws {
        let other = try open(try pngCopy("other.png"), type: "public.png")
        let doc = try open(try pngCopy(), type: "public.png")
        try await invert(doc)
        let engineDoc = try XCTUnwrap(doc.engineDocument)
        ImageDocument.settleTimeout = .milliseconds(100)
        try queueSlowWork(doc, delayMS: 600)  // about 2.4 s
        unsavedAnswers = [.dontSave]
        let watch = MainThreadWatch()
        watch.start()
        try XCTUnwrap(try wc(doc).window).performClose(nil)
        XCTAssertTrue(waitUntil(5) { doc.engineDocument == nil }, "the window closed")
        XCTAssertEqual(unsavedAsked, 1)
        XCTAssertTrue(doc.windowControllers.isEmpty || doc.windowControllers.allSatisfy { $0.window?.isVisible != true })
        // Another image answers at once while the engine finishes the close.
        let start = Date()
        _ = try XCTUnwrap(other.engineDocument).info()
        XCTAssertLessThan(Date().timeIntervalSince(start), 0.5, "another image is not blocked")
        // Quit finishes.
        let recorder = quit([other])
        XCTAssertTrue(waitUntil(5) { recorder.answers.count == 1 }, "Quit finished")
        XCTAssertEqual(recorder.answers, [true])
        watch.stop()
        XCTAssertLessThan(watch.longestGap, 0.5, "the main thread never waited for the engine's close")
        // The engine close finishes in the background.
        XCTAssertTrue(waitUntil(15) { (try? engineDoc.info()) == nil }, "the engine closed the image")
    }

    // MARK: Item 4: Quit waits once per image

    func testQuitWaitsOnceForEachImageThatDoesNotAnswer() async throws {
        let a = try open(try pngCopy("a.png"), type: "public.png")
        let b = try open(try pngCopy("b.png"), type: "public.png")
        ImageDocument.settleTimeout = .seconds(1)
        try queueSlowWork(a, count: 2)  // about 8 s each
        try queueSlowWork(b, count: 2)
        unsavedAnswers = [.dontSave, .dontSave]
        let start = Date()
        let recorder = quit([a, b])
        XCTAssertTrue(waitUntil(15) { recorder.answers.count == 1 }, "Quit finished")
        XCTAssertEqual(recorder.answers, [true])
        XCTAssertEqual(unsavedAsked, 2, "both images were asked about (their state is not known)")
        let times = askedAt.map { $0.timeIntervalSince(start) }
        print("QUIT_ASKED_AT \(times)")
        // One 1 s wait for both (they wait side by side), none after.
        XCTAssertLessThan(times.first ?? 99, 1.7, "the first image was asked after one wait")
        XCTAssertLessThan(times.last ?? 99, 1.9, "the second image did not wait again")
    }

    // MARK: Item 5: a recovered image says why it needs Save As

    func crashAndRecover(_ doc: ImageDocument) async throws -> ImageDocument {
        let engineDoc = try XCTUnwrap(doc.engineDocument)
        let written = await eventually {
            ((try? engineDoc.recoveryStatus())?["acknowledged_revision"] as? NSNumber) != nil
        }
        XCTAssertTrue(written, "a record was written")
        try engineDoc.flushRecovery()
        try engineDoc.detachRecovery(deleteRecord: false)
        for wc in doc.windowControllers { wc.window?.orderOut(nil) }
        doc.close()
        docs.removeAll { $0 === doc }
        let records = RecoveryCenter.pendingRecords()
        XCTAssertEqual(records.count, 1)
        let back = try await ImageDocument.recover(try XCTUnwrap(records.first), controller: NSDocumentController.shared)
        show(back)
        return back
    }

    func testARecoveredAnimatedGIFSaysWhySaveAsksWhere() async throws {
        let url = try writeImageIO("com.compuserve.gif", "anim.gif", frames: 3)
        let doc = try open(url)
        try await invert(doc)
        let back = try await crashAndRecover(doc)
        XCTAssertFalse(back.plainSaveAllowed)
        let message = try wc(back).editor.message ?? ""
        XCTAssertTrue(message.contains("Recovered"), message)
        XCTAssertTrue(message.contains("3 frames"), "the recovered image says why: \(message)")
    }

    func testARecovered16BitPNGSaysWhySaveAsksWhere() async throws {
        let url = try pngCopy("deep.png")
        try sixteenBitPNG(url)
        let doc = try open(url, type: "public.png")
        XCTAssertFalse(doc.plainSaveAllowed)
        try await invert(doc)
        let back = try await crashAndRecover(doc)
        XCTAssertFalse(back.plainSaveAllowed)
        let message = try wc(back).editor.message ?? ""
        XCTAssertTrue(message.contains("Recovered"), message)
        XCTAssertTrue(message.contains("more than 8 bits"), "the recovered image says why: \(message)")
    }

    /// A 4 x 2 16-bit RGBA PNG written over `url`.
    func sixteenBitPNG(_ url: URL) throws {
        let (w, h) = (4, 2)
        var samples = [UInt16](repeating: 0, count: w * h * 4)
        for i in 0..<samples.count { samples[i] = UInt16(truncatingIfNeeded: i * 8191) | 0x00FF }
        for i in stride(from: 3, to: samples.count, by: 4) { samples[i] = 0xFFFF }
        let data = samples.withUnsafeBufferPointer { Data(buffer: $0) }
        let image = try XCTUnwrap(CGImage(
            width: w, height: h, bitsPerComponent: 16, bitsPerPixel: 64, bytesPerRow: w * 8,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue | CGBitmapInfo.byteOrder16Little.rawValue),
            provider: CGDataProvider(data: data as CFData)!, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent))
        try encode(image, "public.png").write(to: url)
    }
}
