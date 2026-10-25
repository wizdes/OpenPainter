// File > Save All (M55, paint.net Ctrl+Alt+S, Option-Cmd-S here): saves every
// open image that has unsaved changes, each through its own Save (so an
// untitled image asks where to save, and a format with options shows them).
// The item is off when no image has unsaved changes, as in paint.net
// (https://www.getpaint.net/doc/latest/FileMenu.html).
import AppKit

extension ImageDocumentController {
    override open func saveAllDocuments(_ sender: Any?) {
        MainActor.assumeIsolated { Self.saveAll(documents) }
    }

    override open func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        if item.action == #selector(saveAllDocuments(_:)) {
            return MainActor.assumeIsolated { Self.canSaveAll(documents) }
        }
        return super.validateUserInterfaceItem(item)
    }

    /// True when at least one of `documents` has unsaved changes.
    @MainActor
    public static func canSaveAll(_ documents: [NSDocument]) -> Bool {
        documents.contains { $0.isDocumentEdited }
    }

    /// Saves each edited document in `documents`, in order. Returns the ones it saved.
    @MainActor @discardableResult
    public static func saveAll(_ documents: [NSDocument]) -> [NSDocument] {
        let edited = documents.filter(\.isDocumentEdited)
        for doc in edited { doc.save(nil) }
        return edited
    }
}
