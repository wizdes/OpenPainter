// The View menu's window side (M56): zoom presets, Rulers, the units, and
// keeping each window in step with the app settings (AppSettings). Zoom In,
// Zoom Out, Zoom to Window and Actual Size are the canvas's own commands
// (M09, MetalCanvasView); this file reuses ZoomPresets and Viewport for the
// rest. paint.net page: https://www.getpaint.net/doc/latest/ViewMenu.html.
import AppKit

/// Selectors the document window controller implements for the View menu.
@MainActor @objc public protocol ViewMenuActions {
    func toggleRulers(_ sender: Any?)
    func zoomToPreset(_ sender: Any?)
}

extension MenuSkeleton {
    /// View > Zoom Presets: paint.net's preset zooms, largest first.
    static var zoomPresetEntries: [Entry] {
        ZoomPresets.values.reversed().map {
            .item(ZoomPresets.label($0), "", [], .selector(#selector(ViewMenuActions.zoomToPreset(_:))))
        }
    }

    /// The zoom a View > Zoom Presets item stands for (its title, "1600%").
    public static func presetZoom(forTitle title: String) -> Double? {
        ZoomPresets.values.first { ZoomPresets.label($0) == title }
    }
}

extension EditorWindowController: ViewMenuActions {
    var settings: AppSettings { workspace.settings }

    /// Wires the canvas, rulers and status bar to the settings. Called once.
    func wireViewSettings() {
        canvas.onPixelGridChange = { [weak self] on in
            guard let self else { return }
            if self.settings.pixelGrid != on { self.settings.pixelGrid = on }
            self.updateViewStates()
        }
        statusBar.onUnit = { [weak self] u in self?.settings.units = u }
        settings.observe(self) { [weak self] in self?.applySettings() }
        viewDocumentChanged()
        applySettings()
    }

    /// Puts the stored settings into this window.
    func applySettings() {
        let s = settings
        if canvas.pixelGrid != s.pixelGrid { canvas.pixelGrid = s.pixelGrid }
        container.rulersVisible = s.rulers
        container.rulerUnit = s.units
        statusBar.unit = s.units
        updateViewStates()
        updateStatus()
    }

    /// The document's resolution and selection for the rulers and status bar.
    /// A failed `doc.resolution` read keeps the last value and is printed, not
    /// reported in the status bar: a report refreshes the editor, which would
    /// call this again.
    func viewDocumentChanged() {
        container.rulerSelection = editor.selection
        let dpi: Double
        do {
            let r = try editor.document.query("doc.resolution") as? [String: Any]
            guard let v = (r?["dpi"] as? NSNumber)?.doubleValue, v.isFinite, v > 0 else { return }
            dpi = v
        } catch {
            FileHandle.standardError.write(Data("OpenPainter: rulers keep \(container.rulerDPI) DPI; doc.resolution failed: \(error)\n".utf8))
            return
        }
        if container.rulerDPI != dpi { container.rulerDPI = dpi }
        if statusBar.dpi != dpi { statusBar.dpi = dpi }
    }

    /// Pixel Grid and Rulers have check marks in the More menu (CommandBar.swift),
    /// which reads them when it opens; the command buttons follow the new state.
    func updateViewStates() {
        refreshCommandBar()
    }

    @objc public func toggleRulers(_ sender: Any?) {
        settings.rulers.toggle()
    }

    @objc public func zoomToPreset(_ sender: Any?) {
        guard let title = (sender as? NSMenuItem)?.title, let z = MenuSkeleton.presetZoom(forTitle: title) else { return }
        var v = canvas.viewport
        v.setZoom(z)
        canvas.setViewport(v)
    }

    /// View > Pixels, Inches, Centimeters (by the item's title).
    func chooseUnits(_ sender: Any?) {
        guard let title = (sender as? NSMenuItem)?.title, let u = MeasurementUnit.allCases.first(where: { $0.title == title })
        else { return }
        settings.units = u
    }

    /// Check marks and enabled states for the View menu items this file adds.
    func validateViewItem(_ item: NSMenuItem) -> Bool? {
        switch item.action {
        case #selector(toggleRulers(_:)):
            item.state = container.rulersVisible ? .on : .off
            return true
        case #selector(zoomToPreset(_:)):
            guard let z = MenuSkeleton.presetZoom(forTitle: item.title) else { return false }
            item.state = abs(canvas.viewport.zoom - z) < 1e-9 ? .on : .off
            return true
        case #selector(EditorMenuActions.setUnits(_:)):
            item.state = MeasurementUnit.allCases.first { $0.title == item.title } == settings.units ? .on : .off
            return true
        default:
            return nil
        }
    }
}
