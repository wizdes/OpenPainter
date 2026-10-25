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
    public private(set) var editor: Editor?
    private var pendingSaveDone: (@MainActor (Bool) -> Void)?
    /// M50: options chosen on the sheet for this image, per format.
    nonisolated(unsafe) public private(set) var chosenSaveOptions: [String: SaveOptions] = [:]
    /// The options of the save in progress (from the sheet).
    nonisolated(unsafe) private var pendingSaveOptions: SaveOptions?
    /// True between a menu or save-panel save and its save(to:) call.
    nonisolated(unsafe) private var saveFromUI = false

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
    }

    /// Edit > Paste into New Image (M34): an untitled image made from an engine
    /// document that already holds the pasted pixels; it saves as .opaint.
    public func startPasted(_ doc: Document, size: CGSize, notes: [String] = []) {
        engineDocument = doc
        pixelSize = size
        importNotes = notes
        fileType = ImageFormats.opaintIdentifier
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
        } else {
            engineDocument = loaded.document
        }
        pixelSize = loaded.pixelSize
        metadata = loaded.metadata
        importNotes = loaded.notes
    }

    /// Revert to Saved: reads the file into the open engine document, then
    /// refreshes the window, panels and marks at once.
    override open func revert(toContentsOf url: URL, ofType typeName: String) throws {
        try super.revert(toContentsOf: url, ofType: typeName)
        MainActor.assumeIsolated {
            if let editor { editor.name = displayName }
            syncEditedState()
        }
    }

    // MARK: Unsaved state

    /// The engine's saved-revision marker.
    /// A closed document (no engine document) has nothing left to save.
    override open var isDocumentEdited: Bool {
        guard let doc = engineDocument else { return false }
        guard let info = try? doc.info() else { return super.isDocumentEdited }
        return info["dirty"] as? Bool ?? false
    }

    /// Brings NSDocument's change count, the window's edited dot and the
    /// editor (strip mark) in line with the engine.
    public func syncEditedState() {
        editor?.refresh()
        updateChangeCount(isDocumentEdited ? .changeDone : .changeCleared)
        for wc in windowControllers { wc.window?.isDocumentEdited = isDocumentEdited }
    }

    // MARK: Save, Save As, Save a Copy

    override open func writableTypes(for saveOperation: NSDocument.SaveOperationType) -> [String] {
        ImageFormats.saveTypes()
    }

    override open func fileNameExtension(forType typeName: String, saveOperation: NSDocument.SaveOperationType) -> String? {
        ImageFormats.fileExtension(typeName) ?? super.fileNameExtension(forType: typeName, saveOperation: saveOperation)
    }

    /// Save on an untitled image or on a format OpenPainter cannot write (such
    /// as Camera RAW) asks where to save, as Save As does.
    override open func save(_ sender: Any?) {
        if fileURL != nil, let type = fileType, !ImageFormats.saveTypes().contains(type) {
            saveAs(sender)
            return
        }
        super.save(sender)
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
            MainActor.assumeIsolated { self?.afterSave() }
            completionHandler(error)
        }
        let type = ImageFormats.saveTypes().contains(typeName)
            ? typeName : (ImageFormats.saveType(forExtension: url.pathExtension) ?? typeName)
        let fromUI = saveFromUI
        saveFromUI = false
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
    }

    override open func write(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                             originalContentsURL absoluteOriginalContentsURL: URL?) throws {
        guard let doc = engineDocument else { throw NSError(domain: "OpenPainter", code: 4) }
        let type = ImageFormats.saveTypes().contains(typeName)
            ? typeName : (ImageFormats.saveType(forExtension: url.pathExtension) ?? typeName)
        let mark = saveOperation == .saveOperation || saveOperation == .saveAsOperation
        do {
            try DocumentSaver.write(doc, to: url, type: type, metadata: metadata.isEmpty ? nil : metadata,
                                    options: pendingSaveOptions ?? saveOptions(for: type), markSaved: mark)
        } catch {
            throw NSError(domain: "OpenPainter", code: 4, userInfo: [
                NSLocalizedDescriptionKey: "“\(url.lastPathComponent)” could not be saved.",
                NSLocalizedFailureReasonErrorKey: error.localizedDescription,
            ])
        }
    }

    // MARK: Close and Quit

    /// The unsaved prompt on Close (and on Quit, through the document
    /// controller). Cancel keeps the window open.
    override open func canClose(withDelegate delegate: Any, shouldClose shouldCloseSelector: Selector?,
                                contextInfo: UnsafeMutableRawPointer?) {
        MainActor.assumeIsolated {
            reviewUnsaved { [weak self] ok in
                guard let self else { return }
                Self.callBack(delegate, shouldCloseSelector, self, ok, contextInfo)
            }
        }
    }

    /// Asks about unsaved work (nothing to ask when there is none) and calls
    /// `done(true)` when the document may close.
    @MainActor
    public func reviewUnsaved(_ done: @escaping @MainActor (Bool) -> Void) {
        guard isDocumentEdited else {
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
        if let url = fileURL, let type = fileType, ImageFormats.saveTypes().contains(type) {
            save(to: url, ofType: type, for: .saveOperation) { [weak self] error in
                MainActor.assumeIsolated {
                    if let error, !Self.isUserCancelled(error) { self?.showError(error) }
                    done(error == nil)
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
            done?(didSave)
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
        }
    }

    override open func showWindows() {
        MainActor.assumeIsolated {
            for case let wc as EditorWindowController in windowControllers { wc.present() }
        }
    }

    override open func close() {
        MainActor.assumeIsolated {
            // The window controller detached the canvas view when its window closed.
            for case let wc as EditorWindowController in windowControllers { wc.canvas.detach() }
        }
        super.close()
        if let doc = engineDocument {
            engineDocument = nil
            do { try doc.close() } catch {
                FileHandle.standardError.write(Data("OpenPainter: closing the document failed: \(error)\n".utf8))
            }
        }
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
        let edited = documents.compactMap { $0 as? ImageDocument }.filter(\.isDocumentEdited)
        review(edited) { ok in
            callDidReviewAll(delegate, didReviewAllSelector, controller, ok, contextInfo)
        }
    }

    /// Asks about each document in turn; a document answered with Save or
    /// Don't Save closes, as AppKit's own review does. Cancel stops at once.
    @MainActor
    public static func review(_ docs: [ImageDocument], done: @escaping @MainActor (Bool) -> Void) {
        guard let first = docs.first else {
            done(true)
            return
        }
        first.reviewUnsaved { ok in
            guard ok else { return done(false) }
            first.close()
            review(Array(docs.dropFirst()), done: done)
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
