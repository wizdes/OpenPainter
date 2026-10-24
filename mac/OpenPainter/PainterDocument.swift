// One open image. The engine reads and decodes the PNG itself (op_doc_open_png).
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

final class PainterDocument: NSDocument {
    private(set) var engineDocument: Document?
    private(set) var pixelSize = CGSize.zero
    nonisolated(unsafe) private static var engine: Engine?

    static func prepareEngine() {
        MainActor.assumeIsolated { engine = AppEngine.shared }
    }

    override class var autosavesInPlace: Bool { false }
    override class func canConcurrentlyReadDocuments(ofType typeName: String) -> Bool { true }

    override func read(from url: URL, ofType typeName: String) throws {
        guard let engine = Self.engine else { throw NSError(domain: "OpenPainter", code: 1) }
        let doc: Document
        do {
            doc = try engine.openPNG(path: url.path)
        } catch {
            throw NSError(
                domain: "OpenPainter", code: 2,
                userInfo: [NSLocalizedDescriptionKey: "The engine could not open “\(url.lastPathComponent)” (\(error))."])
        }
        let info = try doc.info()
        let w = (info["width"] as? NSNumber)?.doubleValue ?? 0
        let h = (info["height"] as? NSNumber)?.doubleValue ?? 0
        guard w > 0, h > 0 else {
            try? doc.close()
            throw NSError(domain: "OpenPainter", code: 3, userInfo: [NSLocalizedDescriptionKey: "The image has no pixels."])
        }
        engineDocument = doc
        pixelSize = CGSize(width: w, height: h)
    }

    override func data(ofType typeName: String) throws -> Data {
        throw NSError(domain: "OpenPainter", code: 4, userInfo: [NSLocalizedDescriptionKey: "Saving comes in a later version."])
    }

    override func makeWindowControllers() {
        guard let doc = engineDocument else { return }
        addWindowController(DocumentWindowController(document: doc, pixelSize: pixelSize))
    }

    override func close() {
        // The window controller detached the canvas view when its window closed.
        for case let wc as DocumentWindowController in windowControllers { wc.detachCanvas() }
        super.close()
        if let doc = engineDocument {
            engineDocument = nil
            do { try doc.close() } catch {
                FileHandle.standardError.write(Data("OpenPainter: closing the document failed: \(error)\n".utf8))
            }
        }
    }
}
