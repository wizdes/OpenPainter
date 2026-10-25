// The Edit menu's clipboard items (M34): Cut, Copy, Copy Merged, Paste, Paste
// into New Layer and Paste into New Image.
//
// paint.net (https://www.getpaint.net/doc/latest/EditMenu.html):
// - Copy puts the selected region of the active layer on the clipboard; the
//   selection stays. With no selection the whole layer is copied.
// - Copy Merged copies all layers (without Image > Flatten), only the selected
//   region when there is a selection.
// - Cut copies, then removes the selected pixels and deselects (one step here:
//   the engine's Erase Selection).
// - Paste puts the image on the active layer, selects it and turns on Move
//   Selected Pixels, so it can be moved, scaled or rotated. When zoomed in, the
//   pixels land in the current view. An image too wide or too tall for the
//   canvas asks whether to resize the canvas (here: Expand canvas, Keep canvas
//   size or Cancel).
// - Paste into New Layer adds a layer above the active one first and makes it
//   active. Paste into New Image makes a new image exactly the size of the
//   clipboard image.
//
// The clipboard holds PNG and TIFF, both with the document's ICC profile. Paste
// reads PNG exactly (the engine's decoder), else TIFF or any image NSImage
// reads (ImageIO). Pixels in another profile are converted to the document's
// profile by the engine (op-color) and the status bar says so.
//
// The paste is one engine move session (`op_paste_begin`): Finish commits one
// step ("Paste" or "Paste into New Layer", the new layer included) and Escape
// leaves the document exactly as it was. Expand canvas is its own Canvas Size
// step before the paste.
import AppKit

/// Edit menu selectors the document window controller implements (M34), next
/// to the standard `cut:`, `copy:` and `paste:`.
@MainActor @objc public protocol ClipboardActions {
    func copyMerged(_ sender: Any?)
    func pasteIntoNewLayer(_ sender: Any?)
    func pasteIntoNewImage(_ sender: Any?)
}

/// The answer to "The image being pasted is larger than the canvas".
public enum OversizePasteChoice: Sendable {
    case expandCanvas, keepCanvasSize, cancel
}

/// Where Copy writes and Paste reads. Tests swap in a private named
/// pasteboard so they never touch the user's clipboard.
@MainActor
public enum ImageClipboard {
    public static var pasteboard = NSPasteboard.general

    public typealias OversizePrompt = @MainActor (
        _ window: NSWindow?, _ image: CGSize, _ canvas: CGSize, _ done: @escaping @MainActor (OversizePasteChoice) -> Void
    ) -> Void
    /// Asks Expand canvas, Keep canvas size or Cancel. Tests replace it; the
    /// default is a sheet on the document window.
    public static var oversizePrompt: OversizePrompt = { window, image, canvas, done in
        showOversizeAlert(window, image: image, canvas: canvas, done)
    }

    /// True when the pasteboard holds an image Paste can read.
    public static var hasImage: Bool {
        pasteboard.availableType(from: [.png, .tiff]) != nil || NSImage.canInit(with: pasteboard)
    }

    /// Writes PNG and TIFF, both with `pixels.icc` embedded.
    public static func write(_ pixels: ClipboardPixels) throws {
        let png = try ImageEncoder.encode(params: ["format": "png"], width: pixels.width, height: pixels.height,
                                          rgba: pixels.rgba, icc: pixels.icc, exif: nil, xmp: nil)
        let tiff = try ImageEncoder.encode(params: ["format": "tiff"], width: pixels.width, height: pixels.height,
                                           rgba: pixels.rgba, icc: pixels.icc, exif: nil, xmp: nil)
        let item = NSPasteboardItem()
        item.setData(png, forType: .png)
        item.setData(tiff, forType: .tiff)
        pasteboard.clearContents()
        guard pasteboard.writeObjects([item]) else { throw ImageClipboardError.cannotWrite }
    }

    /// The pasteboard image as straight RGBA8 with its profile, or nil when
    /// there is no image. PNG first (exact), then TIFF, then anything NSImage reads.
    public static func read() throws -> ClipboardPixels? {
        if let png = pasteboard.data(forType: .png) {
            return try PNGDecoder.decode(png)
        }
        let data: Data
        if let tiff = pasteboard.data(forType: .tiff) {
            data = tiff
        } else if let image = NSImage(pasteboard: pasteboard), let tiff = image.tiffRepresentation {
            data = tiff
        } else {
            return nil
        }
        let decoded = try ImageIOImporter.decode(data: data, name: "clipboard image")
        return ClipboardPixels(width: decoded.width, height: decoded.height, rgba: decoded.rgba, icc: decoded.icc)
    }

    /// `alertSink` takes the alert instead of showing it (the visual check).
    static func showOversizeAlert(_ window: NSWindow?, image: CGSize, canvas: CGSize,
                                  alertSink: ((NSAlert) -> Void)? = nil,
                                  _ done: @escaping @MainActor (OversizePasteChoice) -> Void) {
        let alert = NSAlert()
        alert.messageText = "The image being pasted is larger than the canvas."
        alert.informativeText = "The image is \(Int(image.width)) × \(Int(image.height)) pixels and the canvas is "
            + "\(Int(canvas.width)) × \(Int(canvas.height)). Expand the canvas to fit it?"
        alert.addButton(withTitle: "Expand Canvas")
        alert.addButton(withTitle: "Keep Canvas Size")
        alert.addButton(withTitle: "Cancel")
        let answer: (NSApplication.ModalResponse) -> Void = { r in
            MainActor.assumeIsolated {
                switch r {
                case .alertFirstButtonReturn: done(.expandCanvas)
                case .alertSecondButtonReturn: done(.keepCanvasSize)
                default: done(.cancel)
                }
            }
        }
        if let alertSink { return alertSink(alert) }
        if let window { alert.beginSheetModal(for: window, completionHandler: answer) } else { answer(alert.runModal()) }
    }
}

public enum ImageClipboardError: Error, LocalizedError, Equatable {
    case cannotWrite

    public var errorDescription: String? { "The clipboard could not be written." }
}

public enum ClipboardGeometry {
    /// Where a pasted image of `image` size lands: the top-left of the visible
    /// part of the canvas (`visibleTopLeft`, document pixels), moved left and
    /// up as needed so an image that fits the canvas stays inside it; 0 on an
    /// axis where the image is larger than the canvas.
    public static func pasteOrigin(image: CGSize, canvas: CGSize, visibleTopLeft v: CGPoint) -> (x: Int, y: Int) {
        func axis(_ start: CGFloat, _ size: CGFloat, _ room: CGFloat) -> Int {
            guard size <= room else { return 0 }
            return Int(min(max(0, start.rounded(.up)), room - size))
        }
        return (axis(v.x, image.width, canvas.width), axis(v.y, image.height, canvas.height))
    }
}

extension EditorWindowController: ClipboardActions {
    /// The clipboard items' enabled state; nil for other items.
    func validateClipboardItem(_ item: NSMenuItem) -> Bool? {
        let target = FocusRouter.target(in: window)
        switch item.action {
        case #selector(NSText.copy(_:)), #selector(copyMerged(_:)):
            return target != .dialog && canCopyPixels
        case #selector(NSText.cut(_:)):
            return target != .dialog && editor.selection != nil && canCopyPixels
        case #selector(NSText.paste(_:)), #selector(pasteIntoNewLayer(_:)):
            return target != .dialog && editor.info.width > 0 && ImageClipboard.hasImage
        case #selector(pasteIntoNewImage(_:)):
            return target != .dialog && ImageClipboard.hasImage
        default:
            return nil
        }
    }

    /// Copy has pixels to take: no selection (the whole layer), or a selection
    /// that covers a pixel (the engine's Erase Selection rule).
    var canCopyPixels: Bool {
        editor.info.width > 0 && (editor.selection == nil || editor.isEnabled("selection.erase"))
    }

    @objc public func cut(_ sender: Any?) { cutPixels() }
    @objc public func copy(_ sender: Any?) { copyPixels(merged: false) }
    @objc public func copyMerged(_ sender: Any?) { copyPixels(merged: true) }
    @objc public func paste(_ sender: Any?) { pastePixels(newLayer: false) }
    @objc public func pasteIntoNewLayer(_ sender: Any?) { pastePixels(newLayer: true) }
    @objc public func pasteIntoNewImage(_ sender: Any?) { pasteIntoNewImage() }

    /// Ends open tool sessions so the copy sees their result (a live Magic
    /// Wand selection, a moved selection).
    private func finishSessions() {
        moveTool.finish()
        flood.finish()
    }

    /// Copy (or Copy Merged) to the clipboard. False when there was nothing to copy.
    @discardableResult
    public func copyPixels(merged: Bool) -> Bool {
        finishSessions()
        let name = merged ? "Copy Merged" : "Copy"
        do {
            try ImageClipboard.write(try editor.document.copyPixels(merged: merged))
            return true
        } catch {
            editor.report("\(name) failed: \(error.localizedDescription)")
            return false
        }
    }

    /// Cut: Copy, then Erase Selection (one history step). Nil when nothing was cut.
    @discardableResult
    public func cutPixels() -> UInt64? {
        finishSessions()
        guard editor.selection != nil else {
            editor.report("Cut: nothing is selected")
            return nil
        }
        guard copyPixels(merged: false) else { return nil }
        return editor.run("selection.erase")
    }

    /// Paste or Paste into New Layer. Reads the clipboard, asks about an image
    /// larger than the canvas, then opens the move session. Calls `done` with
    /// true when a paste session opened (tests).
    public func pastePixels(newLayer: Bool, done: (@MainActor (Bool) -> Void)? = nil) {
        let name = newLayer ? "Paste into New Layer" : "Paste"
        let pixels: ClipboardPixels
        do {
            guard let read = try ImageClipboard.read() else {
                editor.report("\(name): the clipboard has no image")
                done?(false)
                return
            }
            pixels = read
        } catch {
            editor.report("\(name) failed: \(error.localizedDescription)")
            done?(false)
            return
        }
        finishSessions()
        if textTool.isEditing { textTool.finish() }
        let canvas = CGSize(width: editor.info.width, height: editor.info.height)
        let size = CGSize(width: pixels.width, height: pixels.height)
        guard size.width > canvas.width || size.height > canvas.height else {
            done?(beginPaste(pixels, newLayer: newLayer, name: name))
            return
        }
        ImageClipboard.oversizePrompt(window, size, canvas) { [weak self] choice in
            guard let self else { return }
            switch choice {
            case .cancel:
                done?(false)
            case .keepCanvasSize:
                done?(self.beginPaste(pixels, newLayer: newLayer, name: name))
            case .expandCanvas:
                // Canvas Size, anchored top left; its own step before the paste.
                self.editor.run("image.canvas_size", [
                    "width": max(Int(canvas.width), pixels.width), "height": max(Int(canvas.height), pixels.height),
                    "anchor": "top_left", "fill": "transparent",
                ])
                done?(self.beginPaste(pixels, newLayer: newLayer, name: name, at: (0, 0)))
            }
        }
    }

    /// Opens the paste session with Move Selected Pixels current.
    private func beginPaste(_ pixels: ClipboardPixels, newLayer: Bool, name: String, at fixed: (Int, Int)? = nil) -> Bool {
        // paint.net turns on Move Selected Pixels; choosing it first keeps the
        // tool change from finishing the new session.
        if workspace.tools.tool != .moveSelectedPixels { workspace.tools.tool = .moveSelectedPixels }
        let start: PasteStart
        let origin: (x: Int, y: Int)
        do {
            // The engine runs commands in order, so an Expand Canvas sent
            // just before has run when the paste starts.
            if let fixed {
                origin = fixed
            } else {
                let v = canvas.viewport
                let topLeft = v.documentPoint(atView: v.clearRect?.origin ?? .zero)
                origin = ClipboardGeometry.pasteOrigin(
                    image: CGSize(width: pixels.width, height: pixels.height),
                    canvas: CGSize(width: editor.info.width, height: editor.info.height), visibleTopLeft: topLeft)
            }
            start = try editor.document.pasteBegin(pixels, x: origin.x, y: origin.y, newLayer: newLayer,
                                                   resampling: workspace.tools.settings.moveResampling.rawValue)
        } catch {
            editor.report("\(name) failed: \(error)")
            return false
        }
        editor.refresh()
        moveTool.adoptPaste(box: CGRect(x: origin.x, y: origin.y, width: pixels.width, height: pixels.height))
        if let from = start.convertedFrom {
            editor.report("\(name): converted from \(from) to \(editor.info.profileDescription)")
        }
        return true
    }

    /// Paste into New Image: a new image the size of the clipboard image,
    /// holding its pixels and its profile. Returns the new document.
    @discardableResult
    public func pasteIntoNewImage(controller: NSDocumentController = .shared) -> ImageDocument? {
        do {
            guard let pixels = try ImageClipboard.read() else {
                editor.report("Paste into New Image: the clipboard has no image")
                return nil
            }
            return try ImageDocumentController.createPastedImage(pixels, engine: workspace.engine,
                                                                 controller: controller)
        } catch {
            editor.report("Paste into New Image failed: \(error.localizedDescription)")
            return nil
        }
    }
}

extension ImageDocumentController {
    /// Makes, adds and shows a new image from clipboard pixels (Paste into New Image).
    @MainActor @discardableResult
    public static func createPastedImage(_ pixels: ClipboardPixels, engine: Engine,
                                         controller: NSDocumentController) throws -> ImageDocument {
        let doc = try engine.importPixels(width: UInt32(pixels.width), height: UInt32(pixels.height),
                                          rgba: pixels.rgba, icc: pixels.icc.map { [UInt8]($0) }, dpi: 96)
        let cls = (controller.documentClass(forType: ImageFormats.opaintIdentifier) as? ImageDocument.Type)
            ?? ImageDocument.self
        let image = cls.init()
        image.startPasted(doc, size: CGSize(width: pixels.width, height: pixels.height))
        controller.addDocument(image)
        image.makeWindowControllers()
        image.showWindows()
        return image
    }
}
