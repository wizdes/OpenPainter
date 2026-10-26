// One open image as an NSDocument (M19, from M17's PainterDocument): New,
// Open, Save, Save As, Save a Copy, Revert, the flatten prompt for flat
// formats, and the unsaved prompts on close and quit. The app's
// PainterDocument is this class (Info.plist names it); tests use it directly.
//
// Unsaved state comes from the engine's saved-revision marker, not a change
// counter: `isDocumentEdited` is `doc.info`'s `dirty`, which is true whenever
// the current history step is not the saved one. Undo back to the saved step
// clears it; a new edit after an undo sets it until the next save.
//
// paint.net behavior (https://www.getpaint.net/doc/latest/FileMenu.html):
// Close and Exit with unsaved changes ask first; a layered image saved to a
// flat format is flattened into the file, and the open image keeps its layers.
import AppKit
import UniformTypeIdentifiers

/// The answer to "Save changes before closing?".
public enum UnsavedChoice: Sendable {
    case save, dontSave, cancel
}

open class ImageDocument: NSDocument, EditorHost {
    /// The engine documents open in. The app sets it at launch; tests set their own.
    nonisolated(unsafe) public static var engine: Engine?
    /// The workspace new windows join (Workspace.shared unless a test sets one).
    nonisolated(unsafe) public static var workspace: (() -> Workspace?) = { MainActor.assumeIsolated { Workspace.shared } }

    public typealias UnsavedPrompt = @MainActor (ImageDocument, @escaping @MainActor (UnsavedChoice) -> Void) -> Void
    public typealias FlattenPrompt = @MainActor (ImageDocument, String, @escaping @MainActor (Bool) -> Void) -> Void
    /// Asks Save, Don't Save or Cancel. Tests replace it; the default is a sheet.
    nonisolated(unsafe) public static var unsavedPrompt: UnsavedPrompt = { doc, done in
        ImageDocument.showUnsavedAlert(doc, done)
    }
    /// Asks Flatten or Cancel before a layered image goes to a flat format.
    nonisolated(unsafe) public static var flattenPrompt: FlattenPrompt = { doc, type, done in
        ImageDocument.showFlattenAlert(doc, type, done)
    }
    public typealias SaveOptionsPrompt = @MainActor (ImageDocument, String, SaveOptions,
                                                     @escaping @MainActor (SaveOptions?) -> Void) -> Void
    /// M50: the save options sheet for a format (nil answer: Cancel). Shown
    /// on Save As and Save a Copy, and on Save for a lossy format whose options were
    /// never chosen for this image, when the save comes from the menu or the
    /// save panel. Tests replace it; the default is SaveOptionsController.
    nonisolated(unsafe) public static var saveOptionsPrompt: SaveOptionsPrompt = { doc, type, options, done in
        ImageDocument.showSaveOptions(doc, type, options, done)
    }
    /// The last options per format (UserDefaults). Tests set their own.
    nonisolated(unsafe) public static var saveOptionsStore = SaveOptionsStore()

    nonisolated(unsafe) public private(set) var engineDocument: Document?
    nonisolated(unsafe) public private(set) var pixelSize = CGSize.zero
    /// EXIF, XMP and GPS from an ImageIO import, written back into ImageIO
    /// saves (M50 adds keep or strip).
    nonisolated(unsafe) public private(set) var metadata = ImageMetadata()
    /// Status-bar notes from the import, such as a 16-bit to 8-bit conversion.
    nonisolated(unsafe) private var importNotes: [String] = []
    /// Fix round 2, item 3: the type of the opened file's content (from its
    /// bytes, not its name), kept in the recovery record.
    nonisolated(unsafe) public private(set) var contentType: String?
    /// Fix round 2, items 1 and 3: plain Save would write over a file that
    /// holds more than OpenPainter can write back (a 16-bit PNG, Camera RAW
    /// named .png, HEIF, PSD), so Save asks where to save, as Save As does,
    /// until the image has been saved to a file it can write.
    nonisolated(unsafe) public private(set) var saveNeedsNewFile = false
    public private(set) var editor: Editor?
    private var pendingSaveDone: (@MainActor (Bool) -> Void)?
    /// M50: options chosen on the sheet for this image, per format.
    nonisolated(unsafe) public private(set) var chosenSaveOptions: [String: SaveOptions] = [:]
    /// The options of the save in progress (from the sheet).
    nonisolated(unsafe) private var pendingSaveOptions: SaveOptions?
    /// True between a menu or save-panel save and its save(to:) call.
    nonisolated(unsafe) private var saveFromUI = false
    /// Reads the revision a save writes (`doc.info`'s `revision`). Tests replace it.
    nonisolated(unsafe) public static var revisionReader: (Document) throws -> UInt64 = defaultRevisionReader
    public static func defaultRevisionReader(_ doc: Document) throws -> UInt64 {
        guard let r = (try doc.info()["revision"] as? NSNumber)?.uint64Value else {
            throw EngineError.internal
        }
        return r
    }

    /// The revision the last write(to:) wrote; marked saved when the save completes.
    nonisolated(unsafe) private var pendingSavedRevision: UInt64?
    /// M57: why recovery could not start (shown in the status bar; retried).
    nonisolated(unsafe) public private(set) var recoveryStartError: String?
    nonisolated(unsafe) private var recoveryOriginal: URL?
    /// M57: this image's recovery record folder (nil: recovery is off).
    nonisolated(unsafe) public private(set) var recoveryDir: URL?
    /// M57: true after a recovery write failed, until one works again.
    nonisolated(unsafe) public private(set) var recoveryFailing = false
    public typealias RecoveryErrorPrompt = @MainActor (ImageDocument, String) -> Void
    /// M57: shown once when recovery copies start failing (disk full). Tests
    /// replace it; the default is a sheet over the image's window.
    nonisolated(unsafe) public static var recoveryErrorPrompt: RecoveryErrorPrompt = { doc, text in
        ImageDocument.showRecoveryAlert(doc, text)
    }

    public required override init() {
        super.init()
    }

    override open class var autosavesInPlace: Bool { false }
    override open class func canConcurrentlyReadDocuments(ofType typeName: String) -> Bool { true }

    // MARK: New and Open

    /// File > New: an untitled one-layer image that saves as .opaint.
    public func startNew(_ settings: NewImageSettings) throws {
        guard let engine = Self.engine else { throw NSError(domain: "OpenPainter", code: 1) }
        let doc: Document
        do {
            doc = try engine.newDocument(width: UInt32(clamping: settings.width), height: UInt32(clamping: settings.height),
                                         dpi: settings.dpi, background: settings.rgba)
        } catch {
            throw NSError(domain: "OpenPainter", code: 5, userInfo: [
                NSLocalizedDescriptionKey: "The image could not be made (\(error)).",
            ])
        }
        engineDocument = doc
        pixelSize = CGSize(width: settings.width, height: settings.height)
        fileType = ImageFormats.opaintIdentifier
        startRecovery(originalURL: nil)
    }

    /// Edit > Paste into New Image (M34): an untitled image made from an engine
    /// document that already holds the pasted pixels; it saves as .opaint.
    /// Release review item 4: it starts edited (no file holds it yet) and
    /// with recovery on, like any new image.
    public func startPasted(_ doc: Document, size: CGSize, notes: [String] = []) {
        engineDocument = doc
        pixelSize = size
        importNotes = notes
        fileType = ImageFormats.opaintIdentifier
        do {
            try doc.markUnsaved()
        } catch {
            importNotes.append("The image could not be marked as unsaved (\(error)).")
        }
        updateChangeCount(.changeDone)
        startRecovery(originalURL: nil)
    }

    /// Open, and Revert (a second read): Revert gives the open engine document
    /// the file's content, so the window, canvas and panels stay.
    override open func read(from url: URL, ofType typeName: String) throws {
        guard let engine = Self.engine else { throw NSError(domain: "OpenPainter", code: 1) }
        let loaded: LoadedImage
        do {
            loaded = try DocumentLoader.open(url, engine: engine)
        } catch {
            throw NSError(domain: "OpenPainter", code: 2, userInfo: [NSLocalizedDescriptionKey: error.localizedDescription])
        }
        if let current = engineDocument {
            defer { try? loaded.document.close() }
            do { try current.replaceContent(with: loaded.document) } catch {
                throw NSError(domain: "OpenPainter", code: 6, userInfo: [
                    NSLocalizedDescriptionKey: "The image could not be reverted (\(error)).",
                ])
            }
            // Fix round 3, item 3: Revert read a new file; its type, depth,
            // frames and color model decide what Save may do from now on.
            applySaveProtection(loaded, url: url)
        } else {
            engineDocument = loaded.document
            applySaveProtection(loaded, url: url)
            startRecovery(originalURL: url)
        }
        pixelSize = loaded.pixelSize
        metadata = loaded.metadata
    }

    /// Fix round 2, items 1 and 3, fix round 3, items 3 and 5: plain Save
    /// asks where to save when the loaded file holds more than OpenPainter
    /// writes back (more than 8 bits per channel, a type it cannot write,
    /// content whose type differs from its name, more than one frame or
    /// page, CMYK or another converted color model). The notes say why.
    private func applySaveProtection(_ loaded: LoadedImage, url: URL) {
        let reasons = ImageFormats.plainSaveReasons(url, contentType: loaded.sourceType)
        contentType = loaded.sourceType
        saveNeedsNewFile = loaded.reducedDepth || !ImageFormats.saveTypes().contains(loaded.sourceType)
            || !reasons.isEmpty
        importNotes = loaded.notes + reasons
    }

    /// Revert to Saved: reads the file into the open engine document, then
    /// refreshes the window, panels and marks at once.
    override open func revert(toContentsOf url: URL, ofType typeName: String) throws {
        // Release review item 1, fix round 2 items 5 and 6: Revert drops
        // every open session in the engine, so the tools drop theirs too
        // (no engine call, no wait); none is left pointing at a session the
        // reverted image no longer has.
        // Fix round 3, item 4: only after the Revert worked. A Revert that
        // failed (the file is gone or cannot be read) left the image and
        // its sessions as they were, so the tools keep theirs.
        try super.revert(toContentsOf: url, ofType: typeName)
        MainActor.assumeIsolated {
            editor?.windowController?.abandonToolSessions()
            if let editor { editor.name = displayName }
            syncEditedState()
            if !importNotes.isEmpty { editor?.report(importNotes.joined(separator: " ")) }
        }
        // Fix round 3, item 3: the recovery record's original type is the
        // reverted file's.
        pointRecoveryAtFile()
    }

    // MARK: Unsaved state

    /// The engine's saved-revision marker. Release review item 1: the
    /// engine also reports dirty while a tool session that changes the image
    /// is open (text, paste or move, a live Magic Wand or Paint Bucket).
    /// A closed document (no engine document) has nothing left to save.
    /// Fix round 2, item 10: `doc.dirty` reads the flag alone (menu and
    /// toolbar validation ask often), not the whole `doc.info`.
    override open var isDocumentEdited: Bool {
        guard let doc = engineDocument else { return false }
        guard let dirty = try? doc.isDirty() else { return super.isDocumentEdited }
        return dirty
    }

    /// Brings NSDocument's change count, the window's edited dot and the
    /// editor (strip mark) in line with the engine.
    public func syncEditedState() {
        editor?.refresh()
        updateChangeCount(isDocumentEdited ? .changeDone : .changeCleared)
        for wc in windowControllers { wc.window?.isDocumentEdited = isDocumentEdited }
    }

    /// Release review item 1: finishes every open tool session (each as its
    /// own history step) so a save writes what the screen shows. The tools
    /// end first; then the engine finishes any session they do not own
    /// (line, shape, gradient) after the commands they sent. Fix round 2,
    /// item 5: the engine does this as a queued command and `done` runs when
    /// it answered, so the main thread never waits behind queued work (a
    /// large fill). `done(false)`: the sessions could not be finished, and
    /// the status bar says why.
    @MainActor
    public func finishOpenSessions(_ done: @escaping @MainActor (Bool) -> Void) {
        editor?.windowController?.finishToolSessions()
        guard let doc = engineDocument else { return done(true) }
        let reply = MainReply<(any Error)?>(main: { [weak self] error in
            if let error { self?.editor?.report("The open tool could not be finished: \(error).") }
            self?.syncEditedState()
            done(error == nil)
        })
        Task.detached {
            do {
                try await doc.finishSessionsQueued()
                reply.send(nil)
            } catch {
                reply.send(error)
            }
        }
    }

    /// How long Close and Quit wait for the engine's queued work. Tests
    /// shorten it.
    nonisolated(unsafe) public static var settleTimeout: Duration = .seconds(600)

    /// Fix round 2, item 2: runs `done` once the engine ran everything
    /// queued so far (a text update or a fill posted just before), so the
    /// edited state read afterwards is current. It never blocks the main
    /// thread. Fix round 3, item 1: `done(false)` when the wait failed or
    /// timed out; the edited state is then not known, and Close and Quit
    /// treat the image as edited.
    @MainActor
    public func afterQueuedWork(_ done: @escaping @MainActor (Bool) -> Void) {
        editor?.windowController?.flushToolUpdates()
        guard let doc = engineDocument else { return done(true) }
        let reply = MainReply<Bool>(main: { [weak self] ok in
            if !ok { self?.editor?.report("The image did not answer in time; its unsaved state may be out of date.") }
            self?.syncEditedState()
            done(ok)
        })
        let timeout = Self.settleTimeout
        Task.detached { reply.send(await doc.settle(timeout: timeout)) }
    }

    // MARK: Save, Save As, Save a Copy

    override open func writableTypes(for saveOperation: NSDocument.SaveOperationType) -> [String] {
        ImageFormats.saveTypes()
    }

    override open func fileNameExtension(forType typeName: String, saveOperation: NSDocument.SaveOperationType) -> String? {
        ImageFormats.fileExtension(typeName) ?? super.fileNameExtension(forType: typeName, saveOperation: saveOperation)
    }

    /// Save on an untitled image or on a format OpenPainter cannot write (such
    /// as Camera RAW) asks where to save, as Save As does. Fix round 2, items
    /// 1 and 3: so does Save on an image whose file holds more than
    /// OpenPainter writes back (16 bits per channel) or whose content is a
    /// type it cannot write, whatever the file's name says.
    override open func save(_ sender: Any?) {
        if fileURL != nil, !plainSaveAllowed {
            saveAs(sender)
            return
        }
        super.save(sender)
    }

    /// True when Save may write the image back to its own file.
    public var plainSaveAllowed: Bool {
        guard let type = fileType, ImageFormats.saveTypes().contains(type) else { return false }
        return !saveNeedsNewFile
    }

    /// The layer count now (1 when unknown).
    public var layerCount: Int {
        ((try? engineDocument?.info())?["layers"] as? [Any])?.count ?? 1
    }

    /// The save panel (Save As, Save a Copy) and File > Save come through
    /// here before save(to:ofType:for:completionHandler:); M50 marks those
    /// saves so the options sheet may show.
    override open func save(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                            delegate: Any?, didSave didSaveSelector: Selector?, contextInfo: UnsafeMutableRawPointer?) {
        saveFromUI = true
        super.save(to: url, ofType: typeName, for: saveOperation, delegate: delegate, didSave: didSaveSelector,
                   contextInfo: contextInfo)
    }

    /// True when a save of `type` by `op` asks for options first: Save As and
    /// Save a Copy always; Save only for a lossy format (JPEG, HEIC, AVIF,
    /// WebP, GIF) until options were chosen for this image. Save to PNG or
    /// TIFF, where every option keeps every pixel, uses the last options
    /// without asking (M19's File > Save tests save PNG with no sheet).
    public func needsSaveOptions(_ type: String, _ op: NSDocument.SaveOperationType) -> Bool {
        guard let kind = SaveOptionKind(type: type) else { return false }
        switch op {
        case .saveAsOperation, .saveToOperation: return true
        case .saveOperation: return kind.isLossy && chosenSaveOptions[type] == nil
        default: return false
        }
    }

    /// The options a save of `type` uses without the sheet: this image's
    /// choice, else the last ones for the format, else the defaults.
    public func saveOptions(for type: String) -> SaveOptions {
        chosenSaveOptions[type] ?? Self.saveOptionsStore.options(type)
    }

    /// Every save comes through here. A menu or save-panel save of a format
    /// with options shows the options sheet first (M50). A layered image going
    /// to a flat format then asks Flatten or Cancel; Cancel saves nothing.
    override open func save(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                            completionHandler: @escaping ((any Error)?) -> Void) {
        let finish: ((any Error)?) -> Void = { [weak self] error in
            self?.pendingSaveOptions = nil
            let written = self?.pendingSavedRevision
            self?.pendingSavedRevision = nil
            MainActor.assumeIsolated {
                // M57 review item 4: the step becomes the saved one only now,
                // after AppKit moved the written file into place; until then the
                // image stays unsaved and its recovery record stays.
                if error == nil, saveOperation == .saveOperation || saveOperation == .saveAsOperation,
                   let self, let doc = self.engineDocument, let written {
                    do {
                        try doc.markSaved(revision: written)
                    } catch EngineError.busy {
                        // Edited during the save: the file holds an older step.
                    } catch {
                        self.editor?.report("The image was saved, but could not be marked as saved: \(error).")
                    }
                }
                if error == nil, saveOperation == .saveAsOperation, let self {
                    // The image now has a file it writes in full.
                    self.saveNeedsNewFile = false
                    self.contentType = typeName
                }
                self?.afterSave()
                // M57: a failed save says why in the status bar too; the
                // image and its history stay as they were.
                if let error, !Self.isUserCancelled(error) { self?.reportSaveFailure(error) }
            }
            completionHandler(error)
        }
        // Release review item 1: Save, Save As, Save a Copy and the Save of
        // Close and Quit write what the screen shows. Fix round 2, item 5:
        // the save goes on when the engine finished them, without blocking
        // the main thread meanwhile.
        let fromUI = saveFromUI
        saveFromUI = false
        MainActor.assumeIsolated {
            finishOpenSessions { [weak self] ok in
                guard let self else { return completionHandler(CocoaError(.userCancelled)) }
                guard ok else {
                    let error = NSError(domain: "OpenPainter", code: 4, userInfo: [
                        NSLocalizedDescriptionKey: "“\(url.lastPathComponent)” could not be saved.",
                        NSLocalizedFailureReasonErrorKey: "The open tool could not be finished.",
                        NSLocalizedRecoverySuggestionErrorKey: "Your changes are still open. The last saved file was not changed.",
                    ])
                    self.reportSaveFailure(error)
                    return completionHandler(error)
                }
                self.continueSave(to: url, ofType: typeName, for: saveOperation, fromUI: fromUI,
                                  finish: finish, completionHandler: completionHandler)
            }
        }
    }

    /// The rest of a save once the open sessions are finished: the options
    /// sheet, the flatten prompt, then AppKit's save.
    private func continueSave(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                              fromUI: Bool, finish: @escaping ((any Error)?) -> Void,
                              completionHandler: @escaping ((any Error)?) -> Void) {
        let type = ImageFormats.saveTypes().contains(typeName)
            ? typeName : (ImageFormats.saveType(forExtension: url.pathExtension) ?? typeName)
        let flattenThenSave = { [weak self] in
            guard let self else { return }
            guard ImageFormats.isFlat(type), self.layerCount > 1, saveOperation != .autosaveElsewhereOperation,
                  saveOperation != .autosaveInPlaceOperation, saveOperation != .autosaveAsOperation
            else {
                self.superSave(to: url, ofType: typeName, for: saveOperation, completionHandler: finish)
                return
            }
            MainActor.assumeIsolated {
                Self.flattenPrompt(self, type) { [weak self] flatten in
                    guard let self else { return }
                    if flatten {
                        self.superSave(to: url, ofType: typeName, for: saveOperation, completionHandler: finish)
                    } else {
                        self.pendingSaveOptions = nil
                        completionHandler(CocoaError(.userCancelled))
                    }
                }
            }
        }
        guard fromUI, needsSaveOptions(type, saveOperation) else {
            flattenThenSave()
            return
        }
        MainActor.assumeIsolated {
            Self.saveOptionsPrompt(self, type, saveOptions(for: type)) { [weak self] chosen in
                guard let self else { return }
                guard let chosen else {
                    completionHandler(CocoaError(.userCancelled))
                    return
                }
                self.chooseSaveOptions(chosen, for: type)
                self.pendingSaveOptions = chosen
                flattenThenSave()
            }
        }
    }

    /// Records the sheet's options for this image and as the format's last ones.
    public func chooseSaveOptions(_ options: SaveOptions, for type: String) {
        chosenSaveOptions[type] = options
        Self.saveOptionsStore.save(options, for: type)
    }

    private func superSave(to url: URL, ofType typeName: String, for op: NSDocument.SaveOperationType,
                           completionHandler: @escaping ((any Error)?) -> Void) {
        super.save(to: url, ofType: typeName, for: op, completionHandler: completionHandler)
    }

    private func afterSave() {
        editor?.name = displayName
        for wc in windowControllers { wc.window?.title = displayName }
        syncEditedState()
        // M57: the next recovery record names the file the image now has.
        pointRecoveryAtFile()
    }

    /// Points the recovery record at the image's file and its content type
    /// (after a save, and after Revert: fix round 3, item 3).
    private func pointRecoveryAtFile() {
        guard let dir = recoveryDir, let doc = engineDocument else { return }
        do {
            try doc.attachRecovery(dir: dir, originalURL: fileURL, originalType: contentType ?? fileType,
                                   timing: RecoveryCenter.timing)
        } catch {
            MainActor.assumeIsolated {
                editor?.report("The recovery copy could not be pointed at the file (\(error)).")
            }
        }
    }

    /// Fix round 2, item 13: AppKit's safe save cannot replace a file that
    /// is a symbolic link ("The file doesn't exist"), so a save through a
    /// link writes the file the link points at itself. The link stays a
    /// link, the target keeps its permissions, and the document keeps the
    /// link as its file.
    override open func writeSafely(to url: URL, ofType typeName: String,
                                   for saveOperation: NSDocument.SaveOperationType) throws {
        let target = Self.saveTarget(url)
        guard target != url else {
            return try super.writeSafely(to: url, ofType: typeName, for: saveOperation)
        }
        // The writers replace the target in one step (a temporary file
        // renamed over it); its permissions are put back afterwards.
        let fm = FileManager.default
        let mode = try? fm.attributesOfItem(atPath: target.path)[.posixPermissions]
        try write(to: target, ofType: typeName, for: saveOperation, originalContentsURL: target)
        if let mode {
            do {
                try fm.setAttributes([.posixPermissions: mode], ofItemAtPath: target.path)
            } catch {
                MainActor.assumeIsolated {
                    editor?.report("The file was saved, but its permissions could not be kept (\(error)).")
                }
            }
        }
    }

    /// `url` itself, or the file at the end of its symbolic links when it is one.
    static func saveTarget(_ url: URL) -> URL {
        guard url.isFileURL, (try? FileManager.default.destinationOfSymbolicLink(atPath: url.path)) != nil else {
            return url
        }
        return url.resolvingSymlinksInPath()
    }

    override open func write(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                             originalContentsURL absoluteOriginalContentsURL: URL?) throws {
        guard let doc = engineDocument else { throw NSError(domain: "OpenPainter", code: 4) }
        let type = ImageFormats.saveTypes().contains(typeName)
            ? typeName : (ImageFormats.saveType(forExtension: url.pathExtension) ?? typeName)
        // The revision being written; the save's completion marks it saved
        // (M57 review item 4: not here, where the file is only a temporary copy).
        // If it cannot be read the save fails and says why (review round 2).
        let revision: UInt64
        do {
            revision = try Self.revisionReader(doc)
        } catch {
            throw NSError(domain: "OpenPainter", code: 4, userInfo: [
                NSLocalizedDescriptionKey: "“\(url.lastPathComponent)” could not be saved.",
                NSLocalizedFailureReasonErrorKey: "The image's state could not be read (\(error)).",
                NSLocalizedRecoverySuggestionErrorKey: "Your changes are still open. The last saved file was not changed.",
            ])
        }
        do {
            try DocumentSaver.write(doc, to: url, type: type, metadata: metadata.isEmpty ? nil : metadata,
                                    options: pendingSaveOptions ?? saveOptions(for: type), markSaved: false)
            pendingSavedRevision = revision
        } catch {
            // M57: the engine's own text (such as "No space left on device"), in plain words.
            let engineText = error is EngineError ? doc.lastFileError() : nil
            let reason = RecoveryCenter.plainReason(engineText ?? Self.describe(error))
            throw NSError(domain: "OpenPainter", code: 4, userInfo: [
                NSLocalizedDescriptionKey: "“\(url.lastPathComponent)” could not be saved.",
                NSLocalizedFailureReasonErrorKey: reason,
                NSLocalizedRecoverySuggestionErrorKey: "Your changes are still open. The last saved file was not changed.",
                NSUnderlyingErrorKey: error,
            ])
        }
    }

    /// An error's text with its underlying POSIX error, if any.
    static func describe(_ error: any Error) -> String {
        let e = error as NSError
        if let under = e.userInfo[NSUnderlyingErrorKey] as? NSError {
            return "\(e.localizedDescription) \(under.localizedDescription) (os error \(under.code))"
        }
        return e.localizedDescription
    }

    // MARK: Recovery (M57)

    /// Turns on recovery records for this image when the app has a recovery
    /// folder. When that fails (the folder cannot be made or locked), the
    /// status bar says so and it is tried again every write interval.
    private func startRecovery(originalURL: URL?) {
        guard RecoveryCenter.root != nil, engineDocument != nil, recoveryDir == nil else { return }
        recoveryOriginal = originalURL
        if !tryStartRecovery() {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.scheduleRecoveryRetry() }
            }
        }
    }

    /// One try; true when recovery is on.
    private func tryStartRecovery() -> Bool {
        guard let root = RecoveryCenter.root, let doc = engineDocument else { return false }
        let dir = root.appendingPathComponent(UUID().uuidString, isDirectory: true)
        do {
            try doc.attachRecovery(dir: dir, originalURL: recoveryOriginal ?? fileURL,
                                   originalType: contentType ?? fileType, timing: RecoveryCenter.timing,
                                   testFailAfterBytes: RecoveryCenter.testFailAfterBytes)
            recoveryDir = dir
            recoveryStartError = nil
            return true
        } catch {
            let text = "Recovery copies are off for this image: the folder \(root.path) could not be used. "
                + "OpenPainter tries again shortly."
            if recoveryStartError == nil {
                FileHandle.standardError.write(Data("OpenPainter: \(text) (\(error))\n".utf8))
            }
            recoveryStartError = text
            return false
        }
    }

    @MainActor
    private func scheduleRecoveryRetry() {
        if let text = recoveryStartError { editor?.report(text) }
        let interval = max(0.05, Double(RecoveryTiming.ms(RecoveryCenter.timing.settle)) / 1000)
        Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] timer in
            MainActor.assumeIsolated {
                guard let self, self.engineDocument != nil, self.recoveryDir == nil else {
                    timer.invalidate()
                    return
                }
                if self.tryStartRecovery() {
                    timer.invalidate()
                    self.editor?.report("Recovery copies are on again for this image.")
                }
            }
        }
    }

    /// The status-bar text for a failed save.
    @MainActor
    func reportSaveFailure(_ error: any Error) {
        let e = error as NSError
        var text = e.localizedDescription
        if let reason = e.localizedFailureReason, !text.contains(reason) { text += " " + reason }
        editor?.report("\(text) Your changes are still open.")
    }

    /// A recovery event from the engine (Workspace forwards it).
    @MainActor
    public func recoveryEvent(_ json: [String: Any]) {
        if json["ok"] as? Bool == false {
            let reason = RecoveryCenter.plainReason(json["error"] as? String ?? "")
            let text = "A recovery copy of “\(displayName ?? "Untitled")” could not be saved. \(reason) "
                + "Your changes are still open."
            editor?.report(text)
            if !recoveryFailing {
                recoveryFailing = true
                Self.recoveryErrorPrompt(self, text)
            }
        } else if recoveryFailing {
            recoveryFailing = false
            editor?.report("Recovery copies are being saved again.")
        }
    }

    /// Tests: called on the main thread when Recover's off-main read is done,
    /// before the window is made.
    @MainActor public static var recoveryOpenedHook: (() -> Void)?

    /// Recover: opens a crash's record as an unsaved image with the original
    /// file as its file (untitled when it had none), and shows its window.
    /// The checksum and decode run off the main thread (M57 review item 7).
    @MainActor @discardableResult
    public static func recover(_ record: RecoveryRecord, controller: NSDocumentController) async throws -> ImageDocument {
        guard let engine = Self.engine else { throw NSError(domain: "OpenPainter", code: 1) }
        let cls = (controller.documentClass(forType: ImageFormats.opaintIdentifier) as? ImageDocument.Type)
            ?? ImageDocument.self
        let timing = RecoveryCenter.timing
        // A record older than its file (saved since, here or by another app)
        // opens as a new untitled image, so it can never replace the newer file.
        let untitled = record.stale
        let opened = try await Task.detached {
            try engine.openRecovery(dir: record.dir, timing: timing, untitled: untitled)
        }.value
        recoveryOpenedHook?()
        let doc = cls.init()
        doc.adoptRecovered(opened, record: record, untitled: untitled)
        controller.addDocument(doc)
        doc.makeWindowControllers()
        doc.showWindows()
        doc.syncEditedState()
        // Fix round 4, item 5: with the notes on why Save asks where.
        doc.editor?.report((["Recovered with the changes that were not saved."] + doc.importNotes).joined(separator: " "))
        return doc
    }

    private func adoptRecovered(_ opened: Document, record: RecoveryRecord, untitled: Bool) {
        engineDocument = opened
        recoveryDir = record.dir
        let info = (try? opened.info()) ?? [:]
        pixelSize = CGSize(width: (info["width"] as? NSNumber)?.doubleValue ?? Double(record.width ?? 0),
                           height: (info["height"] as? NSNumber)?.doubleValue ?? Double(record.height ?? 0))
        // Release review item 5: the metadata the record kept.
        metadata = DocumentLoader.keptMetadata(opened)
        if !untitled, let url = record.originalURL, url.isFileURL {
            fileURL = url
            // Release review item 2, fix round 2 item 3: the type is the one
            // detected from the file's content when it was opened, never one
            // guessed from its name (a Camera RAW file named .png). A type
            // OpenPainter cannot write (Camera RAW, HEIF, PSD, ...), no kept
            // type (an older record), or a file deeper than 8 bits per
            // channel makes Save ask where to save, so the original is never
            // written over.
            let kept = record.originalType
            fileType = kept ?? UTType(filenameExtension: url.pathExtension)?.identifier ?? "public.data"
            contentType = kept
            // Fix round 4, item 5: the notes say why, as Open's do.
            var notes: [String] = []
            if let kept {
                if !ImageFormats.saveTypes().contains(kept) {
                    notes.append("OpenPainter cannot write \(ImageFormats.shortName(kept)) files; "
                        + "Save asks where to save so the original is kept.")
                }
                notes += ImageFormats.plainSaveReasons(url, contentType: kept)
            } else {
                notes.append("This recovery copy does not say what type its file was; "
                    + "Save asks where to save so the original is kept.")
            }
            if ImageFormats.holdsMoreThan8Bits(url) {
                notes.append("The file holds more than 8 bits per channel and OpenPainter keeps 8; "
                    + "Save asks where to save so the original is kept.")
            }
            importNotes = notes
            saveNeedsNewFile = !notes.isEmpty
        } else {
            fileType = ImageFormats.opaintIdentifier
        }
    }

    /// "OpenPainter could not save a recovery copy" over the image's window.
    @MainActor
    static func showRecoveryAlert(_ doc: ImageDocument, _ text: String) {
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Recovery copies cannot be saved"
        alert.informativeText = text + " Save the image to keep your work."
        alert.addButton(withTitle: "OK")
        if let w = doc.windowControllers.first?.window {
            alert.beginSheetModal(for: w) { _ in }
        } else {
            alert.runModal()
        }
    }

    // MARK: Close and Quit

    /// The unsaved prompt on Close (and on Quit, through the document
    /// controller). Cancel keeps the window open.
    override open func canClose(withDelegate delegate: Any, shouldClose shouldCloseSelector: Selector?,
                                contextInfo: UnsafeMutableRawPointer?) {
        MainActor.assumeIsolated {
            // Fix round 3, item 2: Close pressed again while the review
            // runs is answered "do not close" at once; the running review
            // decides, and closes the image if it may.
            if isReviewingUnsaved {
                Self.callBack(delegate, shouldCloseSelector, self, false, contextInfo)
                return
            }
            reviewUnsaved { [weak self] ok in
                guard let self else { return }
                Self.callBack(delegate, shouldCloseSelector, self, ok, contextInfo)
            }
        }
    }

    /// Fix round 3, item 2: the callers waiting for the unsaved-changes
    /// review that runs now (nil when none runs).
    @MainActor private var reviewWaiters: [@MainActor (Bool) -> Void]?

    /// True while an unsaved-changes review (Close or Quit) runs.
    @MainActor public var isReviewingUnsaved: Bool { reviewWaiters != nil }

    /// Asks about unsaved work (nothing to ask when there is none) and calls
    /// `done(true)` when the document may close. Fix round 3, item 2: only
    /// one review runs per image; a request made while one runs (Quit after
    /// Close) gets that review's answer.
    /// Fix round 4, item 4: `unsettled` says the caller's own wait for the
    /// image's queued work just failed or timed out (Quit's first step), so
    /// the review asks at once instead of waiting again.
    @MainActor
    public func reviewUnsaved(unsettled: Bool = false, _ done: @escaping @MainActor (Bool) -> Void) {
        if reviewWaiters != nil {
            reviewWaiters?.append(done)
            return
        }
        reviewWaiters = [done]
        runReview(unsettled: unsettled) { [weak self] ok in
            guard let self else { return done(ok) }
            let waiters = self.reviewWaiters ?? []
            self.reviewWaiters = nil
            for waiter in waiters { waiter(ok) }
        }
    }

    @MainActor
    private func runReview(unsettled: Bool = false, _ done: @escaping @MainActor (Bool) -> Void) {
        // Fix round 2, item 2: decide once the engine ran what was queued
        // (a text update or a fill sent just before Close). Fix round 3,
        // item 1: when that wait failed, the state is not known: ask.
        if unsettled { return reviewSettled(assumeEdited: true, done) }
        afterQueuedWork { [weak self] settled in
            guard let self else { return done(true) }
            self.reviewSettled(assumeEdited: !settled, done)
        }
    }

    @MainActor
    private func reviewSettled(assumeEdited: Bool, _ done: @escaping @MainActor (Bool) -> Void) {
        guard isDocumentEdited || assumeEdited else {
            done(true)
            return
        }
        Self.unsavedPrompt(self) { [weak self] choice in
            guard let self else { return done(false) }
            switch choice {
            case .cancel: done(false)
            case .dontSave: done(true)
            case .save: self.saveBeforeClosing(done)
            }
        }
    }

    @MainActor
    private func saveBeforeClosing(_ done: @escaping @MainActor (Bool) -> Void) {
        if let url = fileURL, let type = fileType, plainSaveAllowed {
            save(to: url, ofType: type, for: .saveOperation) { [weak self] error in
                MainActor.assumeIsolated {
                    if let error, !Self.isUserCancelled(error) { self?.showError(error) }
                    guard error == nil, let self else { return done(false) }
                    self.closeAfterSave(done)
                }
            }
        } else {
            pendingSaveDone = done
            runModalSavePanel(for: .saveAsOperation, delegate: self,
                              didSave: #selector(savePanelFinished(_:didSave:contextInfo:)), contextInfo: nil)
        }
    }

    @objc private func savePanelFinished(_ doc: NSDocument, didSave: Bool, contextInfo: UnsafeMutableRawPointer?) {
        MainActor.assumeIsolated {
            let done = pendingSaveDone
            pendingSaveDone = nil
            guard let done else { return }
            guard didSave else { return done(false) }
            closeAfterSave(done)
        }
    }

    /// Release review item 9: after Close's save, an edit that landed after
    /// the save read the image (so the file holds an older step) keeps the
    /// image edited, and Close asks again instead of dropping it.
    @MainActor
    private func closeAfterSave(_ done: @escaping @MainActor (Bool) -> Void) {
        if isDocumentEdited {
            editor?.report("The image changed while it was being saved.")
            runReview(done)
        } else {
            done(true)
        }
    }

    @MainActor
    private func showError(_ error: any Error) {
        if let w = windowControllers.first?.window {
            presentError(error, modalFor: w, delegate: nil, didPresent: nil, contextInfo: nil)
        } else {
            presentError(error)
        }
    }

    static func isUserCancelled(_ error: any Error) -> Bool {
        let e = error as NSError
        return e.domain == NSCocoaErrorDomain && e.code == NSUserCancelledError
    }

    /// Calls an Objective-C `document:shouldClose:contextInfo:`-style callback.
    static func callBack(_ delegate: Any?, _ selector: Selector?, _ doc: NSDocument, _ ok: Bool,
                         _ context: UnsafeMutableRawPointer?) {
        guard let target = delegate as? NSObject, let selector, target.responds(to: selector) else { return }
        typealias Fn = @convention(c) (NSObject, Selector, NSDocument, Bool, UnsafeMutableRawPointer?) -> Void
        let imp = target.method(for: selector)
        unsafeBitCast(imp, to: Fn.self)(target, selector, doc, ok, context)
    }

    // MARK: Prompts (defaults)

    /// "Save changes to “name” before closing?" with Save, Don't Save and Cancel.
    @MainActor
    static func showUnsavedAlert(_ doc: ImageDocument, _ done: @escaping @MainActor (UnsavedChoice) -> Void) {
        let alert = NSAlert()
        alert.messageText = "Save changes to “\(doc.displayName ?? "Untitled")” before closing?"
        alert.informativeText = "If you don't save, your changes will be lost."
        alert.addButton(withTitle: "Save")
        let dont = alert.addButton(withTitle: "Don't Save")
        dont.keyEquivalent = "d"
        dont.keyEquivalentModifierMask = [.command]
        alert.addButton(withTitle: "Cancel")
        let map: (NSApplication.ModalResponse) -> UnsavedChoice = { r in
            switch r {
            case .alertFirstButtonReturn: .save
            case .alertSecondButtonReturn: .dontSave
            default: .cancel
            }
        }
        if let w = doc.windowControllers.first?.window {
            w.makeKeyAndOrderFront(nil)
            alert.beginSheetModal(for: w) { r in MainActor.assumeIsolated { done(map(r)) } }
        } else {
            done(map(alert.runModal()))
        }
    }

    /// The save options sheet over the image's window (M50).
    @MainActor
    static func showSaveOptions(_ doc: ImageDocument, _ type: String, _ options: SaveOptions,
                                _ done: @escaping @MainActor (SaveOptions?) -> Void) {
        let pixels: SavePixels
        do {
            guard let engineDoc = doc.engineDocument else { throw NSError(domain: "OpenPainter", code: 4) }
            pixels = try DocumentSaver.savePixels(engineDoc)
        } catch {
            doc.showError(NSError(domain: "OpenPainter", code: 4, userInfo: [
                NSLocalizedDescriptionKey: "The save options could not be shown.",
                NSLocalizedFailureReasonErrorKey: error.localizedDescription,
            ]))
            done(nil)
            return
        }
        let sheet = SaveOptionsController(type: type, options: options, pixels: pixels,
                                          metadata: doc.metadata.isEmpty ? nil : doc.metadata)
        openSaveOptions = sheet
        sheet.onClose = { result in
            MainActor.assumeIsolated {
                openSaveOptions = nil
                done(result)
            }
        }
        sheet.present(on: doc.windowControllers.first?.window)
    }

    /// The save options sheet while it is open.
    @MainActor public private(set) static var openSaveOptions: SaveOptionsController?

    /// paint.net's flatten prompt: Flatten or Cancel.
    @MainActor
    static func showFlattenAlert(_ doc: ImageDocument, _ type: String, _ done: @escaping @MainActor (Bool) -> Void) {
        let alert = NSAlert()
        alert.messageText = "Flatten image?"
        alert.informativeText = "\(ImageFormats.shortName(type)) files hold one layer, so the saved file is flattened. "
            + "The open image keeps its layers."
        alert.addButton(withTitle: "Flatten")
        alert.addButton(withTitle: "Cancel")
        if let w = doc.windowControllers.first?.window {
            alert.beginSheetModal(for: w) { r in MainActor.assumeIsolated { done(r == .alertFirstButtonReturn) } }
        } else {
            done(alert.runModal() == .alertFirstButtonReturn)
        }
    }

    // MARK: Windows

    override open func makeWindowControllers() {
        MainActor.assumeIsolated {
            guard let doc = engineDocument, let workspace = Self.workspace() else { return }
            let editor = Editor(document: doc, pixelSize: pixelSize, name: displayName)
            editor.host = self
            self.editor = editor
            workspace.add(editor)
            let wc = EditorWindowController(editor: editor, workspace: workspace)
            addWindowController(wc)
            if !importNotes.isEmpty { editor.report(importNotes.joined(separator: " ")) }
            if let text = recoveryStartError { editor.report(text) }
        }
    }

    override open func showWindows() {
        MainActor.assumeIsolated {
            for case let wc as EditorWindowController in windowControllers { wc.present() }
        }
    }

    /// Fix round 3, item 2: Close and Quit may both end in a close.
    nonisolated(unsafe) private var didClose = false

    /// Fix round 5, item 2: true once the document closed (or has no
    /// engine image any more); it has nothing left to ask about.
    @MainActor var isClosed: Bool { didClose || engineDocument == nil }

    override open func close() {
        guard !didClose else { return }
        didClose = true
        MainActor.assumeIsolated {
            // The window controller detached the canvas view when its window closed.
            for case let wc as EditorWindowController in windowControllers { wc.canvas.detach() }
        }
        super.close()
        if let doc = engineDocument {
            engineDocument = nil
            Self.closeEngineDocument(doc)
        }
    }

    /// Fix round 4, item 3: engine closes still running.
    nonisolated private static let engineCloses = DispatchGroup()

    /// Closes `doc` in the engine off the main thread. The engine's close
    /// waits for the image's queued work (after a wait that timed out, that
    /// work may be stuck), so the window closes at once and neither the
    /// main thread nor other images wait for it. It stops the image's
    /// recovery record here, on the calling thread, as the engine's
    /// close did before, so a Quit right after never finds the record.
    static func closeEngineDocument(_ doc: Document) {
        do { try doc.stopRecovery(deleteRecord: true) } catch {
            FileHandle.standardError.write(Data("OpenPainter: stopping the recovery copy failed: \(error)\n".utf8))
        }
        engineCloses.enter()
        DispatchQueue.global(qos: .userInitiated).async {
            defer { engineCloses.leave() }
            do { try doc.close() } catch {
                FileHandle.standardError.write(Data("OpenPainter: closing the document failed: \(error)\n".utf8))
            }
        }
    }

    /// Waits up to `timeout` for the engine closes still running (Quit
    /// calls it). False when some were still running at the limit.
    public static func waitForEngineCloses(timeout: TimeInterval) -> Bool {
        engineCloses.wait(timeout: .now() + timeout) == .success
    }

    // MARK: EditorHost

    public func closeEditor() {
        // The standard flow: asks to save a modified image first.
        for wc in windowControllers { wc.window?.performClose(nil) }
    }

    public func dirtyChanged(_ dirty: Bool) {
        updateChangeCount(dirty ? .changeDone : .changeCleared)
    }
}

/// Fix round 2, items 2 and 5: hands an engine answer, awaited off the main
/// thread, back to the main thread as a run-loop block. A run-loop block
/// also runs inside a nested run loop (a sheet, a tracking loop or a test
/// that spins the run loop), where a main-actor task would wait.
final class MainReply<Value>: @unchecked Sendable {
    private let main: @MainActor (Value) -> Void

    init(main: @escaping @MainActor (Value) -> Void) {
        self.main = main
    }

    func send(_ value: Value) {
        let main = main
        nonisolated(unsafe) let value = value
        RunLoop.main.perform(inModes: [.common]) { MainActor.assumeIsolated { main(value) } }
        CFRunLoopWakeUp(CFRunLoopGetMain())
    }
}

/// The app's document controller: File > New shows the New Image dialog, and
/// Quit reviews each image with unsaved work in turn (Save, Don't Save,
/// Cancel). Cancel stops the quit and keeps the app and that window open.
///
/// The work is in static functions that take the controller, because AppKit
/// keeps one document controller per process: a second
/// `NSDocumentController()` returns the first, so tests cannot make their own.
open class ImageDocumentController: NSDocumentController {
    /// The New Image dialog while it is open.
    public private(set) static var newImageDialog: NewImageController?
    /// Called after File > New shows its image (the app closes its start window).
    public static var onNewImage: (@MainActor (ImageDocument) -> Void)?
    /// The last File > New failure. Shown in an alert unless `presentsErrors` is false (tests).
    public private(set) static var lastNewImageError: (any Error)?
    public static var presentsErrors = true

    override open func newDocument(_ sender: Any?) {
        MainActor.assumeIsolated { Self.showNewImageDialog(controller: self) }
    }

    override open func reviewUnsavedDocuments(withAlertTitle title: String?, cancellable: Bool, delegate: Any?,
                                              didReviewAllSelector: Selector?, contextInfo: UnsafeMutableRawPointer?) {
        MainActor.assumeIsolated {
            Self.reviewUnsaved(documents, controller: self, delegate: delegate, didReviewAllSelector: didReviewAllSelector,
                               contextInfo: contextInfo)
        }
    }

    @MainActor
    public static func showNewImageDialog(controller: NSDocumentController) {
        if let open = newImageDialog {
            open.window?.makeKeyAndOrderFront(nil)
            return
        }
        let secondary = ImageDocument.workspace()?.tools.secondary ?? DocColor(r: 255, g: 255, b: 255)
        let dialog = NewImageController(settings: NewImageSettings(secondary: secondary))
        dialog.onFinish = { [weak controller] settings in
            MainActor.assumeIsolated {
                newImageDialog = nil
                guard let settings, let controller else { return }
                do {
                    lastNewImageError = nil
                    let doc = try createNewImage(settings, controller: controller)
                    onNewImage?(doc)
                } catch {
                    lastNewImageError = error
                    if presentsErrors { controller.presentError(error) }
                }
            }
        }
        newImageDialog = dialog
        dialog.present()
    }

    /// Makes, adds and shows a new image.
    @MainActor @discardableResult
    public static func createNewImage(_ settings: NewImageSettings, controller: NSDocumentController) throws -> ImageDocument {
        let cls = (controller.documentClass(forType: ImageFormats.opaintIdentifier) as? ImageDocument.Type)
            ?? ImageDocument.self
        let doc = cls.init()
        try doc.startNew(settings)
        controller.addDocument(doc)
        doc.makeWindowControllers()
        doc.showWindows()
        return doc
    }

    /// Quit's review: asks about each image with unsaved work in turn, then
    /// calls `documentController:didReviewAll:contextInfo:` on `delegate`.
    @MainActor
    public static func reviewUnsaved(_ documents: [NSDocument], controller: NSDocumentController, delegate: Any?,
                                     didReviewAllSelector: Selector?, contextInfo: UnsafeMutableRawPointer?) {
        // Fix round 4, item 6: Quit joins a review that already runs (Close
        // pressed first) at once, before any wait. Joining later could miss
        // its answer: the review may end while Quit waits for queued work,
        // and Quit would then ask a second time.
        let images = documents.compactMap { $0 as? ImageDocument }
        let running = images.filter(\.isReviewingUnsaved)
        joinRunningReviews(running) { ok in
            guard ok else { return callDidReviewAll(delegate, didReviewAllSelector, controller, false, contextInfo) }
            let rest = images.filter { image in !running.contains { $0 === image } }
            // Fix round 2, item 2: each image's queued work runs first, so an
            // edit sent just before Quit counts.
            settle(rest) { edited, unsettled in
                review(edited, unsettled: unsettled) { ok in
                    callDidReviewAll(delegate, didReviewAllSelector, controller, ok, contextInfo)
                }
            }
        }
    }

    /// Fix round 4, item 6: waits for each running review in turn and gets
    /// its answer; an image answered with Save or Don't Save closes.
    /// Cancel stops at once.
    @MainActor
    static func joinRunningReviews(_ docs: [ImageDocument], _ done: @escaping @MainActor (Bool) -> Void) {
        guard let first = docs.first else { return done(true) }
        first.reviewUnsaved { ok in
            guard ok else { return done(false) }
            first.close()
            joinRunningReviews(Array(docs.dropFirst()), done)
        }
    }

    /// Calls `done` once each one's queued engine work ran, with the
    /// documents that may hold unsaved work: edited ones, ones whose wait
    /// failed or timed out (fix round 3, item 1), and ones whose review
    /// already runs (fix round 3, item 2: Quit joins it).
    @MainActor
    /// Fix round 4, item 4: `done` also gets the documents whose wait
    /// failed or timed out, so their review does not wait again.
    static func settle(_ docs: [ImageDocument],
                       _ done: @escaping @MainActor ([ImageDocument], Set<ObjectIdentifier>) -> Void) {
        var left = docs.count
        guard left > 0 else { return done([], []) }
        var unknown = Set<ObjectIdentifier>()
        for d in docs {
            d.afterQueuedWork { ok in
                if !ok { unknown.insert(ObjectIdentifier(d)) }
                left -= 1
                if left == 0 {
                    done(docs.filter { unknown.contains(ObjectIdentifier($0)) || $0.isDocumentEdited || $0.isReviewingUnsaved },
                         unknown)
                }
            }
        }
    }

    /// Asks about each document in turn; a document answered with Save or
    /// Don't Save closes, as AppKit's own review does. Cancel stops at once.
    /// Fix round 5, item 2: a document closed meanwhile (the user closed it
    /// while Quit asked about another one) is skipped, not asked about.
    @MainActor
    public static func review(_ docs: [ImageDocument], unsettled: Set<ObjectIdentifier> = [],
                              done: @escaping @MainActor (Bool) -> Void) {
        guard let first = docs.first else {
            done(true)
            return
        }
        if first.isClosed {
            return review(Array(docs.dropFirst()), unsettled: unsettled, done: done)
        }
        first.reviewUnsaved(unsettled: unsettled.contains(ObjectIdentifier(first))) { ok in
            guard ok else { return done(false) }
            first.close()
            review(Array(docs.dropFirst()), unsettled: unsettled, done: done)
        }
    }

    static func callDidReviewAll(_ delegate: Any?, _ selector: Selector?, _ controller: NSDocumentController,
                                 _ ok: Bool, _ context: UnsafeMutableRawPointer?) {
        guard let target = delegate as? NSObject, let selector, target.responds(to: selector) else { return }
        typealias Fn = @convention(c) (NSObject, Selector, NSDocumentController, Bool, UnsafeMutableRawPointer?) -> Void
        let imp = target.method(for: selector)
        unsafeBitCast(imp, to: Fn.self)(target, selector, controller, ok, context)
    }
}
