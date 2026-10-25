// M57 autosave and crash recovery on the app side (docs/ARCHITECTURE.md,
// "Recovery records"). Each open image gets a record folder
// `<root>/<doc-uuid>/`; the engine writes the records itself after edits
// settle. At launch, `offerAtLaunch` lists the records a crash left and shows
// one sheet with Recover and Discard for each.
import AppKit

public enum RecoveryCenter {
    /// Where records live. nil turns recovery off (the default, so tests
    /// never write to the user's folder); the app sets `defaultRoot` at launch.
    nonisolated(unsafe) public static var root: URL?
    nonisolated(unsafe) public static var timing = RecoveryTiming()
    /// Tests: every record write fails with "No space left on device" after this many bytes.
    nonisolated(unsafe) public static var testFailAfterBytes: Int?

    /// ~/Library/Application Support/OpenPainter/Recovery
    public static var defaultRoot: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("OpenPainter/Recovery", isDirectory: true)
    }

    /// The records no running app owns: images that did not close cleanly.
    public static func pendingRecords() -> [RecoveryRecord] {
        guard let root else { return [] }
        do {
            return try Engine.scanRecovery(root: root)
        } catch {
            FileHandle.standardError.write(Data("OpenPainter: cannot list recovery records: \(error)\n".utf8))
            return []
        }
    }

    /// The recovery sheet while it is open.
    @MainActor public private(set) static var sheet: RecoverySheetController?

    /// At launch: when records exist, shows the sheet (over `window` when
    /// given). `onRecovered` runs after each recovered image opens, and
    /// `onFinished` when the sheet closes.
    @MainActor @discardableResult
    public static func offerAtLaunch(controller: NSDocumentController, on window: NSWindow? = nil,
                                     onRecovered: (@MainActor (ImageDocument) -> Void)? = nil,
                                     onFinished: (@MainActor () -> Void)? = nil) -> RecoverySheetController? {
        let records = pendingRecords()
        guard !records.isEmpty, sheet == nil else { return nil }
        let s = RecoverySheetController(records: records)
        s.onRecover = { record in
            let doc = try ImageDocument.recover(record, controller: controller)
            onRecovered?(doc)
        }
        s.onDiscard = { record in try Engine.discardRecovery(dir: record.dir) }
        s.onClose = {
            sheet = nil
            onFinished?()
        }
        sheet = s
        s.present(on: window)
        return s
    }

    /// Plain words for an engine or system file error, for the status bar
    /// and the error sheet.
    public static func plainReason(_ text: String) -> String {
        let t = text.lowercased()
        if t.contains("no space left") || t.contains("os error 28") || t.contains("out of space")
            || t.contains("not enough space") {
            return "The disk is full. Free some space, then save again."
        }
        if t.contains("permission denied") || t.contains("os error 13") || t.contains("read-only")
            || t.contains("os error 30") {
            return "OpenPainter is not allowed to write there. Choose another folder."
        }
        if t.contains("no such file") || t.contains("os error 2)") {
            return "The folder does not exist any more."
        }
        return "The file could not be written (\(text))."
    }
}
