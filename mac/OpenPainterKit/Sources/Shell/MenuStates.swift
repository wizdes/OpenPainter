// Menus complete (M55): the Layers items that M49 built in the engine (Import
// From File, Move Layer to Top and to Bottom), and the enabled rules the
// engine's own `enabled` flags do not cover. Every item is off exactly when it
// cannot act:
// - no document: the document window is not in the responder chain, so AppKit
//   turns its items off; Adjustments and Effects ask EffectMenuTarget, which
//   finds no document window;
// - a sheet or dialog is up: document commands are off;
// - Flatten: off with one layer; Move Layer to Top/Bottom: off when the layer
//   is already there; Go to Layer and Merge Layer Down: off with no layer
//   above or below (EditorWindowController and the engine);
// - Save All: off when no image has unsaved changes (SaveAll.swift).
import AppKit
import UniformTypeIdentifiers

/// Layers menu selectors the document window controller implements (M55).
@MainActor @objc public protocol LayerEdgeMenuActions {
    func importLayerFromFile(_ sender: Any?)
    func moveLayerToTop(_ sender: Any?)
    func moveLayerToBottom(_ sender: Any?)
}

extension EditorWindowController: LayerEdgeMenuActions {
    /// Layers > Import From File…: an open panel, then the file as a new layer
    /// above the active one (the engine converts its color profile).
    @objc public func importLayerFromFile(_ sender: Any?) {
        guard let window, window.attachedSheet == nil else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.image]
        panel.message = "Choose an image to add as a new layer"
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.importLayer(from: url)
        }
    }

    /// Adds the image file at `url` as a new layer (tests call this directly).
    public func importLayer(from url: URL) {
        editor.run("layer.import_from_file", ["path": url.path])
    }

    @objc public func moveLayerToTop(_ sender: Any?) {
        let count = editor.info.layers.count, active = editor.info.activeLayer
        guard active < count - 1 else { return }
        editor.run("layer.move", ["from": active, "to": count - 1])
    }

    @objc public func moveLayerToBottom(_ sender: Any?) {
        let active = editor.info.activeLayer
        guard active > 0 else { return }
        editor.run("layer.move", ["from": active, "to": 0])
    }

    /// The M55 enabled rules, or nil when the item is not one of them.
    func menuState(_ item: NSMenuItem, busy: Bool) -> Bool? {
        let count = editor.info.layers.count, active = editor.info.activeLayer
        switch item.action {
        case #selector(importLayerFromFile(_:)): return !busy
        case #selector(moveLayerToTop(_:)): return !busy && active < count - 1
        case #selector(moveLayerToBottom(_:)): return !busy && active > 0
        case #selector(runCommand(_:)):
            guard let id = item.representedObject as? String else { return false }
            // paint.net turns Flatten off for a one-layer image; the engine allows it.
            if id == "image.flatten" { return !busy && count > 1 && editor.isEnabled(id) }
            return nil
        default:
            return nil
        }
    }
}
