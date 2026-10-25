// One open image. The engine reads and decodes PNG itself (op_doc_open_png);
// HEIC, HEIF, AVIF and Camera RAW come through ImageIO (M17, DocumentLoader).
// Save As writes HEIC, and AVIF where this macOS can encode it.
import AppKit
import OpenPainterKit

/// The app's one engine. Created on first use.
enum AppEngine {
    @MainActor static let shared: Engine = {
        do {
            return try Engine()
        } catch {
            fatalError("OpenPainter: the engine could not start: \(error)")
        }
    }()
}

final class PainterDocument: NSDocument, EditorHost {
    private(set) var engineDocument: Document?
    private(set) var pixelSize = CGSize.zero
    nonisolated(unsafe) private static var engine: Engine?

    static func prepareEngine() {
        MainActor.assumeIsolated { engine = AppEngine.shared }
    }

    override class var autosavesInPlace: Bool { false }
    override class func canConcurrentlyReadDocuments(ofType typeName: String) -> Bool { true }

    /// EXIF, XMP and GPS from an ImageIO import, kept for M50's save options
    /// (the engine has no metadata slot).
    private(set) var metadata = ImageMetadata()
    /// Status-bar notes from the import, such as a 16-bit to 8-bit conversion.
    private var importNotes: [String] = []

    override func read(from url: URL, ofType typeName: String) throws {
        guard let engine = Self.engine else { throw NSError(domain: "OpenPainter", code: 1) }
        let loaded: LoadedImage
        do {
            loaded = try DocumentLoader.open(url, engine: engine)
        } catch {
            throw NSError(domain: "OpenPainter", code: 2, userInfo: [NSLocalizedDescriptionKey: error.localizedDescription])
        }
        engineDocument = loaded.document
        pixelSize = loaded.pixelSize
        metadata = loaded.metadata
        importNotes = loaded.notes
    }

    /// Save As offers HEIC, and AVIF only where ImageIO can encode it. Other
    /// formats come with M19 and M50.
    override func writableTypes(for saveOperation: NSDocument.SaveOperationType) -> [String] {
        ImageFormats.exportTypes().map(\.identifier)
    }

    override func data(ofType typeName: String) throws -> Data {
        guard let doc = engineDocument,
              ImageFormats.exportTypes().contains(where: { $0.identifier == typeName })
        else {
            throw NSError(domain: "OpenPainter", code: 4, userInfo: [NSLocalizedDescriptionKey: "Saving this format comes in a later version."])
        }
        return try DocumentLoader.export(doc, type: typeName, metadata: metadata)
    }

    override func makeWindowControllers() {
        guard let doc = engineDocument, let workspace = Workspace.shared else { return }
        let editor = Editor(document: doc, pixelSize: pixelSize, name: displayName)
        editor.host = self
        workspace.add(editor)
        let wc = EditorWindowController(editor: editor, workspace: workspace)
        addWindowController(wc)
        if !importNotes.isEmpty { editor.report(importNotes.joined(separator: " ")) }
    }

    override func showWindows() {
        for case let wc as EditorWindowController in windowControllers { wc.present() }
    }

    override func close() {
        // The window controller detached the canvas view when its window closed.
        for case let wc as EditorWindowController in windowControllers { wc.canvas.detach() }
        super.close()
        if let doc = engineDocument {
            engineDocument = nil
            do { try doc.close() } catch {
                FileHandle.standardError.write(Data("OpenPainter: closing the document failed: \(error)\n".utf8))
            }
        }
    }

    // MARK: EditorHost

    func closeEditor() {
        // The standard flow: asks to save a modified image first.
        for wc in windowControllers { wc.window?.performClose(nil) }
    }

    func dirtyChanged(_ dirty: Bool) {
        updateChangeCount(dirty ? .changeDone : .changeCleared)
    }
}
