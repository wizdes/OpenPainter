// Drag and drop open (M17): files dropped on the canvas or on the image strip
// open as documents, one after another in drop order. Files OpenPainter cannot
// open are named in one status-bar message and nothing opens for them. (Import
// as Layer comes later.)
//
// The canvas area (CanvasContainerView, so the scroll bars count too) and the
// strip each register for file URLs. While a usable drag is over them they show
// a 2 pt accent outline, the strip's active-item outline.
import AppKit
import UniformTypeIdentifiers

/// Opens one file and calls `done` when it has opened or failed. The app passes
/// NSDocumentController's open; tests pass their own.
public typealias FileOpener = @MainActor (URL, @escaping @MainActor ((any Error)?) -> Void) -> Void

@MainActor
public enum FileDrop {
    /// Opens the files in order; only one opens at a time, so documents arrive
    /// in drop order even when one file is slower to decode than the next.
    /// Unsupported files are named in one message on `editor` (or the active
    /// editor) and open nothing. Returns the files that will open.
    @discardableResult
    public static func open(_ urls: [URL], in workspace: Workspace, reportTo editor: Editor? = nil) -> [URL] {
        let (good, bad) = split(urls)
        if !bad.isEmpty {
            (editor ?? workspace.active)?.report(message(for: bad))
        }
        guard let opener = workspace.fileOpener else {
            if !good.isEmpty { (editor ?? workspace.active)?.report("Opening files is not set up.") }
            return []
        }
        openInOrder(good, opener: opener) { url, error in
            (editor ?? workspace.active)?.report("Could not open \(url.lastPathComponent): \(error.localizedDescription)")
        }
        return good
    }

    /// Splits files into those OpenPainter opens and the rest.
    public static func split(_ urls: [URL]) -> (supported: [URL], unsupported: [URL]) {
        var good: [URL] = [], bad: [URL] = []
        for u in urls {
            if ImageFormats.route(for: u) == .unsupported { bad.append(u) } else { good.append(u) }
        }
        return (good, bad)
    }

    /// The one status-bar message for files that cannot open.
    public static func message(for unsupported: [URL]) -> String {
        let names = unsupported.map(\.lastPathComponent)
        let list = ListFormatter.localizedString(byJoining: names)
        let what = names.count == 1 ? "this file" : "these \(names.count) files"
        return "Cannot open \(what): \(list). OpenPainter opens \(ImageFormats.openSummary)."
    }

    static func openInOrder(_ urls: [URL], opener: @escaping FileOpener,
                            failed: @escaping @MainActor (URL, any Error) -> Void) {
        guard let first = urls.first else { return }
        let rest = Array(urls.dropFirst())
        opener(first) { error in
            if let error { failed(first, error) }
            openInOrder(rest, opener: opener, failed: failed)
        }
    }

    /// File URLs on a drag pasteboard, in drop order.
    public static func fileURLs(on pasteboard: NSPasteboard) -> [URL] {
        let objects = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
        return (objects as? [URL]) ?? []
    }

    static let types: [NSPasteboard.PasteboardType] = [.fileURL]

    // MARK: Drag destination behaviour shared by the canvas area and the strip

    static func entered(_ view: NSView, _ info: any NSDraggingInfo) -> NSDragOperation {
        guard !fileURLs(on: info.draggingPasteboard).isEmpty else { return [] }
        DropHighlightView.show(on: view)
        return .copy
    }

    static func exited(_ view: NSView) {
        DropHighlightView.hide(on: view)
    }

    static func perform(_ view: NSView, _ info: any NSDraggingInfo, workspace: Workspace?, editor: Editor?) -> Bool {
        DropHighlightView.hide(on: view)
        let urls = fileURLs(on: info.draggingPasteboard)
        guard let workspace, !urls.isEmpty else { return false }
        open(urls, in: workspace, reportTo: editor)
        return true  // an unsupported drop was still handled: it shows the message
    }
}

/// The 2 pt accent outline shown while files are dragged over a drop target.
/// It never takes mouse events.
final class DropHighlightView: NSView {
    static let identifier = NSUserInterfaceItemIdentifier("FileDropHighlight")

    static func show(on view: NSView) {
        guard highlight(on: view) == nil else { return }
        let h = DropHighlightView(frame: view.bounds)
        h.identifier = identifier
        h.autoresizingMask = [.width, .height]
        view.addSubview(h, positioned: .above, relativeTo: nil)
    }

    static func hide(on view: NSView) {
        highlight(on: view)?.removeFromSuperview()
    }

    static func highlight(on view: NSView) -> DropHighlightView? {
        view.subviews.first { $0.identifier == identifier } as? DropHighlightView
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        Theme.accent.setStroke()
        let path = NSBezierPath(rect: bounds.insetBy(dx: 1, dy: 1))
        path.lineWidth = 2
        path.stroke()
    }
}

extension FileDrop {
    /// Lets the window's canvas area and image strip take dropped files.
    static func register(_ wc: EditorWindowController) {
        wc.container.registerForDraggedTypes(types)
        wc.strip.registerForDraggedTypes(types)
    }
}

extension CanvasContainerView {
    public override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        FileDrop.entered(self, sender)
    }

    public override func draggingExited(_ sender: (any NSDraggingInfo)?) { FileDrop.exited(self) }

    public override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        let wc = window?.windowController as? EditorWindowController
        return FileDrop.perform(self, sender, workspace: wc?.workspace, editor: wc?.editor)
    }
}

extension ImageStripView {
    public override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        FileDrop.entered(self, sender)
    }

    public override func draggingExited(_ sender: (any NSDraggingInfo)?) { FileDrop.exited(self) }

    public override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        let wc = window?.windowController as? EditorWindowController
        return FileDrop.perform(self, sender, workspace: workspace, editor: wc?.editor ?? workspace?.active)
    }
}
