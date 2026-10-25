// One open image. The document logic lives in OpenPainterKit's ImageDocument
// (M19: New, Open, Save, Save As, Save a Copy, Revert, the flatten prompt and
// the unsaved prompts), so the XCTests can drive it; Info.plist names this class.
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

final class PainterDocument: ImageDocument {
    static func prepareEngine() {
        MainActor.assumeIsolated { ImageDocument.engine = AppEngine.shared }
    }
}
