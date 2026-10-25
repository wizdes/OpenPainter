// M57 crash recovery and save failures in the app: an open image writes a
// recovery record after edits settle, a clean save and a close delete it, the
// launch sheet lists what a crash left (Recover opens the image unsaved with
// its changes and its original file; Discard deletes the copy), and a failed
// save or recovery write says why in plain words in the status bar and a
// sheet while the work stays open. The process-kill tests are in the engine
// (op-cli/tests/recovery_kill.rs); here a "crash" is a writer stopped without
// deleting its record.
import AppKit
@testable import OpenPainterKit
import UniformTypeIdentifiers
import XCTest

@MainActor
final class RecoveryTests: XCTestCase {
    var fx: ShellFixture!
    var dir: URL!
    var root: URL!
    var docs: [ImageDocument] = []
    var savedEngine: Engine?
    var savedWorkspace: (() -> Workspace?)!
    var savedPrompt: ImageDocument.RecoveryErrorPrompt!
    var prompts: [String] = []
    var lockedDirs: [URL] = []

    override func setUp() async throws {
        fx = try ShellFixture()
        dir = try ImageIOFixtures.directory()
        root = dir.appendingPathComponent("Recovery", isDirectory: true)
        savedEngine = ImageDocument.engine
        savedWorkspace = ImageDocument.workspace
        savedPrompt = ImageDocument.recoveryErrorPrompt
        ImageDocument.engine = fx.engine
        let ws = fx.workspace
        ImageDocument.workspace = { ws }
        ImageDocument.recoveryErrorPrompt = { [unowned self] _, text in prompts.append(text) }
        RecoveryCenter.root = root
        RecoveryCenter.timing = RecoveryTiming(settle: .milliseconds(50), maxInterval: .seconds(5))
        RecoveryCenter.testFailAfterBytes = nil
    }

    override func tearDown() async throws {
        for d in docs {
            for wc in d.windowControllers { wc.window?.orderOut(nil) }
            d.close()
        }
        docs.removeAll()
        RecoveryCenter.sheet?.later(nil)
        RecoveryCenter.root = nil
        RecoveryCenter.timing = RecoveryTiming()
        RecoveryCenter.testFailAfterBytes = nil
        ImageDocument.engine = savedEngine
        ImageDocument.workspace = savedWorkspace
        ImageDocument.recoveryErrorPrompt = savedPrompt
        for d in lockedDirs {
            try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: d.path)
        }
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
    func open(_ url: URL) throws -> ImageDocument {
        let doc = try ImageDocument(contentsOf: url, ofType: UTType.png.identifier)
        docs.append(doc)
        doc.makeWindowControllers()
        for wc in doc.windowControllers { wc.window?.isReleasedWhenClosed = false }
        doc.showWindows()
        return doc
    }

    func run(_ doc: ImageDocument, _ command: String) async throws {
        let status = try await XCTUnwrap(doc.editor).runAndWait(command)
        XCTAssertEqual(status, "ok", command)
    }

    /// Waits while letting main-actor work run (engine events reach the
    /// workspace through a main-actor task, which `waitUntil` does not run).
    func eventually(_ seconds: Double = 10, _ condition: @escaping () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return condition()
    }

    func status(_ doc: ImageDocument) throws -> [String: Any] {
        try XCTUnwrap(doc.engineDocument).recoveryStatus()
    }

    func revision(_ doc: ImageDocument) throws -> UInt64 {
        try XCTUnwrap((XCTUnwrap(doc.engineDocument).info()["revision"] as? NSNumber)?.uint64Value)
    }

    func manifest(_ doc: ImageDocument) throws -> URL {
        try XCTUnwrap(doc.recoveryDir).appendingPathComponent("manifest.json")
    }

    func saveAndWait(_ doc: ImageDocument, to url: URL, type: String,
                     _ op: NSDocument.SaveOperationType) -> (any Error)? {
        var result: (any Error)?? = .none
        doc.save(to: url, ofType: type, for: op) { result = .some($0) }
        XCTAssertTrue(waitUntil(20) { result != nil }, "save finished")
        return result ?? nil
    }

    /// Edits, waits for the record, then stops the writer without deleting
    /// the record (a crash) and closes the image. Returns the edited pixels.
    func crash(_ doc: ImageDocument) async throws -> [UInt8] {
        try await run(doc, "image.invert_colors")
        let rev = try revision(doc)
        XCTAssertTrue(waitUntil { (try? self.status(doc)["acknowledged_revision"] as? NSNumber)?.uint64Value == rev })
        let pixels = try XCTUnwrap(doc.engineDocument).pixels()
        try XCTUnwrap(doc.engineDocument).detachRecovery(deleteRecord: false)
        for wc in doc.windowControllers { wc.window?.orderOut(nil) }
        doc.close()
        docs.removeAll { $0 === doc }
        return pixels
    }

    // MARK: Records

    func testAnOpenImageWritesARecordAndACleanSaveAndCloseDeleteIt() async throws {
        let url = try pngCopy()
        let doc = try open(url)
        let recordDir = try XCTUnwrap(doc.recoveryDir)
        XCTAssertEqual(recordDir.deletingLastPathComponent().standardizedFileURL, root.standardizedFileURL)
        XCTAssertNotNil(UUID(uuidString: recordDir.lastPathComponent), "the folder is the image's UUID")
        // A clean image has no record.
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertFalse(FileManager.default.fileExists(atPath: try manifest(doc).path))
        try await run(doc, "image.invert_colors")
        let rev = try revision(doc)
        XCTAssertTrue(waitUntil { (try? self.status(doc)["acknowledged_revision"] as? NSNumber)?.uint64Value == rev })
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: try manifest(doc))) as? [String: Any]
        XCTAssertEqual(json?["original_url"] as? String, url.absoluteString)
        XCTAssertEqual((json?["revision"] as? NSNumber)?.uint64Value, rev)
        // A live image's record is not offered.
        XCTAssertTrue(RecoveryCenter.pendingRecords().isEmpty)
        // Save As .opaint: the record goes, and the next one names the new file.
        let saved = dir.appendingPathComponent("saved.opaint")
        XCTAssertNil(saveAndWait(doc, to: saved, type: ImageFormats.opaintIdentifier, .saveAsOperation))
        XCTAssertTrue(waitUntil { !FileManager.default.fileExists(atPath: (try? self.manifest(doc).path) ?? "") })
        try await run(doc, "image.invert_colors")
        XCTAssertTrue(waitUntil { FileManager.default.fileExists(atPath: (try? self.manifest(doc).path) ?? "") })
        XCTAssertTrue(waitUntil {
            let j = (try? Data(contentsOf: self.manifest(doc))).flatMap { try? JSONSerialization.jsonObject(with: $0) }
            return (j as? [String: Any])?["original_url"] as? String == saved.absoluteString
        })
        // Close deletes the folder.
        for wc in doc.windowControllers { wc.window?.orderOut(nil) }
        doc.close()
        docs.removeAll()
        XCTAssertFalse(FileManager.default.fileExists(atPath: recordDir.path))
    }

    func testRecoverFromTheSheetOpensTheImageUnsavedWithItsChangesAndOriginalFile() async throws {
        let url = try pngCopy()
        let original = try Data(contentsOf: url)
        let expected = try await crash(try open(url))
        let pending = RecoveryCenter.pendingRecords()
        XCTAssertEqual(pending.count, 1)
        XCTAssertEqual(pending.first?.originalURL, url)
        XCTAssertEqual(pending.first?.width, 512)
        var recovered: [ImageDocument] = []
        let sheet = try XCTUnwrap(RecoveryCenter.offerAtLaunch(controller: NSDocumentController.shared,
                                                               onRecovered: { recovered.append($0) }))
        XCTAssertEqual(sheet.rows.count, 1)
        XCTAssertEqual(sheet.rows[0].nameLabel.stringValue, "plasma.png")
        XCTAssertTrue(sheet.rows[0].detailLabel.stringValue.contains("512 × 384 px"))
        XCTAssertTrue(sheet.rows[0].recoverButton.isEnabled)
        sheet.rows[0].recoverButton.performClick(nil)
        let doc = try XCTUnwrap(recovered.first)
        docs.append(doc)
        XCTAssertNil(RecoveryCenter.sheet, "the sheet closes after the last record")
        XCTAssertTrue(doc.isDocumentEdited, "a recovered image is unsaved")
        XCTAssertEqual(doc.fileURL?.standardizedFileURL, url.standardizedFileURL)
        XCTAssertEqual(try XCTUnwrap(doc.engineDocument).pixels(), expected)
        XCTAssertEqual(try Data(contentsOf: url), original, "recovering does not touch the original file")
        XCTAssertTrue(try XCTUnwrap(doc.editor).message?.contains("Recovered") == true)
        XCTAssertTrue(waitUntil { (try? XCTUnwrap(doc.windowControllers.first?.window).isDocumentEdited) == true })
        // The recovered image owns its record now; closing it deletes it.
        XCTAssertTrue(RecoveryCenter.pendingRecords().isEmpty)
        let recordDir = try XCTUnwrap(doc.recoveryDir)
        XCTAssertTrue(FileManager.default.fileExists(atPath: recordDir.appendingPathComponent("manifest.json").path))
        for wc in doc.windowControllers { wc.window?.orderOut(nil) }
        doc.close()
        docs.removeAll()
        XCTAssertFalse(FileManager.default.fileExists(atPath: recordDir.path))
    }

    func testTheSheetMarksADamagedRecordAndDiscardDeletesEachRecord() async throws {
        _ = try await crash(try open(try pngCopy("a.png")))
        _ = try await crash(try open(try pngCopy("b.png")))
        var records = RecoveryCenter.pendingRecords()
        XCTAssertEqual(records.count, 2)
        // Damage one snapshot.
        let bad = records[1].dir
        let snapshot = try XCTUnwrap(try FileManager.default.contentsOfDirectory(atPath: bad.path)
            .first { $0.hasPrefix("snapshot-") })
        let path = bad.appendingPathComponent(snapshot)
        var bytes = try Data(contentsOf: path)
        bytes[bytes.count / 2] ^= 0xFF
        try bytes.write(to: path)
        records = RecoveryCenter.pendingRecords()
        let sheet = try XCTUnwrap(RecoveryCenter.offerAtLaunch(controller: NSDocumentController.shared))
        XCTAssertEqual(sheet.rows.count, 2)
        let damaged = try XCTUnwrap(sheet.rows.first { $0.record.dir == bad })
        XCTAssertFalse(damaged.recoverButton.isEnabled)
        XCTAssertTrue(damaged.detailLabel.stringValue.contains("damaged"))
        XCTAssertTrue(damaged.discardButton.isEnabled)
        damaged.discardButton.performClick(nil)
        XCTAssertEqual(sheet.rows.count, 1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: bad.path))
        sheet.rows[0].discardButton.performClick(nil)
        XCTAssertNil(RecoveryCenter.sheet)
        XCTAssertTrue(RecoveryCenter.pendingRecords().isEmpty)
    }

    func testDecideLaterKeepsTheRecordsForTheNextLaunch() async throws {
        _ = try await crash(try open(try pngCopy()))
        let sheet = try XCTUnwrap(RecoveryCenter.offerAtLaunch(controller: NSDocumentController.shared))
        sheet.laterButton.performClick(nil)
        XCTAssertNil(RecoveryCenter.sheet)
        XCTAssertEqual(RecoveryCenter.pendingRecords().count, 1)
    }

    // MARK: Failures

    func testAFailedSaveSaysWhyInTheStatusBarAndTheSheetAndKeepsTheWork() async throws {
        let folder = dir.appendingPathComponent("locked", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let target = folder.appendingPathComponent("work.opaint")
        let doc = try open(try pngCopy())
        XCTAssertNil(saveAndWait(doc, to: target, type: ImageFormats.opaintIdentifier, .saveAsOperation))
        let good = try Data(contentsOf: target)
        try await run(doc, "image.invert_colors")
        let pixels = try XCTUnwrap(doc.engineDocument).pixels()
        let steps = try XCTUnwrap(doc.engineDocument).info()["history_labels"] as? [String]
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: folder.path)
        lockedDirs.append(folder)
        // NSDocument checks folder permission itself before it asks the
        // image to write, so its own error ("You don't have permission")
        // shows; the status bar repeats it once.
        let error = try XCTUnwrap(saveAndWait(doc, to: target, type: ImageFormats.opaintIdentifier, .saveOperation))
        let message = try XCTUnwrap(doc.editor?.message)
        XCTAssertTrue(message.contains((error as NSError).localizedDescription), message)
        XCTAssertTrue(message.hasSuffix("Your changes are still open."), message)
        XCTAssertFalse(message.contains("permission. You don’t have permission"), "said once: \(message)")
        // The engine's own failure, as a write that gets past that check
        // meets it, in plain words: the text NSDocument's error sheet shows.
        XCTAssertThrowsError(try doc.write(to: target, ofType: ImageFormats.opaintIdentifier, for: .saveOperation,
                                           originalContentsURL: target)) { error in
            let e = error as NSError
            XCTAssertEqual(e.localizedDescription, "“work.opaint” could not be saved.")
            XCTAssertEqual(e.localizedFailureReason, "OpenPainter is not allowed to write there. Choose another folder.")
            XCTAssertEqual(e.localizedRecoverySuggestion,
                           "Your changes are still open. The last saved file was not changed.")
        }
        // Nothing is lost: same pixels, same history, still unsaved; the old file is whole.
        XCTAssertTrue(doc.isDocumentEdited)
        XCTAssertEqual(try XCTUnwrap(doc.engineDocument).pixels(), pixels)
        XCTAssertEqual(try XCTUnwrap(doc.engineDocument).info()["history_labels"] as? [String], steps)
        XCTAssertEqual(try Data(contentsOf: target), good)
        XCTAssertNoThrow(try fx.engine.openOpaint(path: target.path).close())
    }

    func testAFailedRecoveryWriteShowsInTheStatusBarAndOneSheet() async throws {
        RecoveryCenter.testFailAfterBytes = 200
        let doc = try open(try pngCopy())
        try await run(doc, "image.invert_colors")
        let prompted = await eventually { self.prompts.count == 1 }
        XCTAssertTrue(prompted, "status \((try? self.status(doc)) ?? [:])")
        let prompt = try XCTUnwrap(prompts.first)
        XCTAssertTrue(prompt.contains("The disk is full"), prompt)
        XCTAssertTrue(doc.recoveryFailing)
        let message = try XCTUnwrap(doc.editor?.message)
        XCTAssertTrue(message.contains("recovery copy") && message.contains("The disk is full"), message)
        // A second failure updates the status bar but asks no second time.
        try await run(doc, "image.invert_colors")
        let before = (try status(doc)["failures"] as? NSNumber)?.intValue ?? 0
        let again = await eventually { ((try? self.status(doc)["failures"] as? NSNumber)?.intValue ?? 0) > before }
        XCTAssertTrue(again)
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertEqual(prompts.count, 1)
        XCTAssertTrue(doc.isDocumentEdited)
    }

    func testPlainReasons() {
        XCTAssertEqual(RecoveryCenter.plainReason("cannot save /x.opaint: file I/O: No space left on device (os error 28)"),
                       "The disk is full. Free some space, then save again.")
        XCTAssertEqual(RecoveryCenter.plainReason("cannot write /x: Permission denied (os error 13)"),
                       "OpenPainter is not allowed to write there. Choose another folder.")
        XCTAssertEqual(RecoveryCenter.plainReason("cannot write /x: No such file or directory (os error 2)"),
                       "The folder does not exist any more.")
        XCTAssertEqual(RecoveryCenter.plainReason("odd"), "The file could not be written (odd).")
    }

    // MARK: Look

    /// Sizes from the SPEC (24 pt targets, 12 pt rows text, 11 pt secondary);
    /// with OP_RENDER_DIR set, writes the sheet in light and dark.
    func testTheSheetLayoutAndRender() throws {
        func record(_ name: String?, valid: Bool) -> RecoveryRecord {
            RecoveryRecord(json: [
                "uuid": UUID().uuidString, "dir": dir.appendingPathComponent(UUID().uuidString).path, "valid": valid,
                "problem": valid ? NSNull() : "the snapshot does not match its checksum",
                "revision": 12, "original_url": name.map { "file:///Users/me/Pictures/\($0)" } ?? NSNull(),
                "time_unix": 1_761_400_000.0, "width": 1200, "height": 760, "layers": 3, "snapshot_bytes": 1000,
            ])
        }
        let sheet = RecoverySheetController(records: [
            record("Harbor at dusk.opaint", valid: true), record(nil, valid: true), record("Logo.png", valid: false),
        ])
        XCTAssertEqual(sheet.rows.map(\.nameLabel.stringValue), ["Harbor at dusk.opaint", "Untitled", "Logo.png"])
        for row in sheet.rows {
            XCTAssertEqual(row.frame.height, RecoverySheetController.rowHeight)
            XCTAssertGreaterThanOrEqual(row.recoverButton.frame.height, Theme.target)
            XCTAssertGreaterThanOrEqual(row.discardButton.frame.height, Theme.target)
            XCTAssertEqual(row.nameLabel.font?.pointSize, 12)
            XCTAssertEqual(row.detailLabel.font?.pointSize, 11)
            XCTAssertEqual(row.detailLabel.textColor, Theme.muted)
        }
        XCTAssertTrue(sheet.rows[0].detailLabel.stringValue.contains("1200 × 760 px · 3 layers"))
        XCTAssertGreaterThanOrEqual(sheet.laterButton.frame.height, Theme.target)
        guard let out = ProcessInfo.processInfo.environment["OP_RENDER_DIR"] else { return }
        let view = try XCTUnwrap(sheet.window?.contentView)
        for name in [NSAppearance.Name.aqua, .darkAqua] {
            view.appearance = NSAppearance(named: name)
            let size = view.bounds.size
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            rep.size = size
            NSAppearance(named: name)!.performAsCurrentDrawingAppearance {
                view.layoutSubtreeIfNeeded()
                view.cacheDisplay(in: view.bounds, to: rep)
            }
            let file = URL(fileURLWithPath: out)
                .appendingPathComponent("recovery-sheet-\(name == .aqua ? "light" : "dark").png")
            try XCTUnwrap(rep.representation(using: .png, properties: [:])).write(to: file)
        }
    }
}
