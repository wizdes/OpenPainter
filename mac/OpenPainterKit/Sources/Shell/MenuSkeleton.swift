// The full menu bar in paint.net 5.1 order: File, Edit, View, Image, Layers,
// Adjustments, Effects, Window, Help, after the Mac app menu. Items come from
// paint.net's menu pages (https://www.getpaint.net/doc/latest/FileMenu.html and
// the Edit, View, Image, Layers, Adjustments and Effects pages). Adjustments
// and Effects are built from the engine's effect registry, in registry order.
// Items with no action are not built yet and stay disabled.
//
// M55: tests/catalog/paintnet-5.1-menus.toml lists every paint.net 5.1 item in
// order with its Windows and Mac shortcuts, our command and its status;
// MenuCatalogTests and tools/catalog/compare-menus.py check this menu bar
// against it. Effects follow paint.net's submenu order, not registry order.
// MenuIcons.swift holds the SF Symbols; MenuStates.swift the enabled rules.
//
// Shortcuts: paint.net's Ctrl becomes Cmd and Alt becomes Option. Where a
// paint.net key clashes with a standard Mac key, the Mac key wins and the
// paint.net item has no shortcut:
//   Rotate 90° Clockwise  Ctrl+H        (Cmd+H is Hide OpenPainter)
//   Merge Layer Down      Ctrl+M        (Cmd+M is Minimize)
//   Toggle Layer Visibility Ctrl+,      (Cmd+, is Settings)
//   Rotate / Zoom         Ctrl+Shift+Z  (Cmd+Shift+Z is Redo on the Mac)
//   Redo                  Ctrl+Y        -> Cmd+Shift+Z
//   Exit                  Alt+F4        -> OpenPainter > Quit (Cmd+Q)
//   Erase Selection       Delete        -> Delete (backspace key)
//   Fill Selection        Backspace     -> Shift+Delete
//   Copy Selection        Ctrl+Alt+Shift+C -> Option+Shift+Cmd+C (M29)
//   Paste Selection > Replace  Ctrl+Alt+Shift+V -> Option+Shift+Cmd+V (M29)
//   Tools/History/Layers/Colors  F5-F8 -> F5-F8, and Option+Cmd+1 to 4
import AppKit

/// Selectors the document window controller implements (found through the
/// responder chain).
@MainActor @objc public protocol EditorMenuActions {
    func runCommand(_ sender: Any?)
    func runEffect(_ sender: Any?)
    func addLayer(_ sender: Any?)
    func goToLayer(_ sender: Any?)
    func zoomToSelection(_ sender: Any?)
    func setUnits(_ sender: Any?)
}

/// Selectors the canvas implements.
@MainActor @objc public protocol CanvasMenuActions {
    func zoomIn(_ sender: Any?)
    func zoomOut(_ sender: Any?)
    func zoomToWindow(_ sender: Any?)
    func actualSize(_ sender: Any?)
    func togglePixelGrid(_ sender: Any?)
}

@objc protocol StandardEditActions {
    func undo(_ sender: Any?)
    func redo(_ sender: Any?)
}

public enum MenuSkeleton {
    enum Act {
        case selector(Selector)
        /// An engine command id, run with no parameters.
        case command(String)
        case effect(String)
        case panel(PanelKind)
        case resetPanels
        /// Not built yet: present and disabled.
        case none
    }

    indirect enum Entry {
        case item(String, String, NSEvent.ModifierFlags, Act)
        case separator
        case submenu(String, [Entry])
    }

    static let cmd: NSEvent.ModifierFlags = [.command]
    static let shiftCmd: NSEvent.ModifierFlags = [.command, .shift]
    static let optCmd: NSEvent.ModifierFlags = [.command, .option]
    static let opt: NSEvent.ModifierFlags = [.option]
    static let optShiftCmd: NSEvent.ModifierFlags = [.command, .option, .shift]

    static func key(_ scalar: Int) -> String { String(UnicodeScalar(UInt16(scalar)).map(Character.init) ?? " ") }
    static let deleteKey = String(Character(UnicodeScalar(8)))

    static func i(_ title: String, _ k: String = "", _ m: NSEvent.ModifierFlags = cmd, _ a: Act = .none) -> Entry {
        .item(title, k, k.isEmpty ? [] : m, a)
    }

    /// paint.net Adjustments, in its menu order, with Mac shortcuts.
    static let paintNetAdjustments: [(String, String, NSEvent.ModifierFlags)] = [
        ("Auto-Level", "l", shiftCmd), ("Black and White", "g", shiftCmd), ("Brightness / Contrast", "t", shiftCmd),
        ("Curves", "m", shiftCmd), ("Exposure", "", []), ("Highlights / Shadows", "", []),
        ("Hue / Saturation", "u", shiftCmd), ("Invert Alpha", "i", optCmd), ("Invert Colors", "i", shiftCmd),
        ("Levels", "l", cmd), ("Posterize", "p", shiftCmd), ("Sepia", "e", shiftCmd),
        ("Temperature and Tint", "", []),
    ]

    /// paint.net Effects submenus and their items.
    static let paintNetEffects: [(String, [String])] = [
        ("Artistic", ["Ink Sketch", "Oil Painting", "Pencil Sketch"]),
        ("Blurs", ["Bokeh Blur", "Fragment Blur", "Gaussian Blur", "Median Blur", "Motion Blur", "Radial Blur",
                   "Sketch Blur", "Square Blur", "Surface Blur", "Unfocus", "Zoom Blur"]),
        ("Color", ["Quantize"]),
        ("Distort", ["Bulge", "Crystalize", "Dents", "Frosted Glass", "Morphology", "Pixelate", "Polar Inversion",
                     "Tile Reflection", "Twist"]),
        ("Noise", ["Add Noise", "Reduce Noise"]),
        ("Object", ["Drop Shadow"]),
        ("Photo", ["Glow", "Red Eye Removal", "Sharpen", "Soften Portrait", "Straighten", "Vignette"]),
        ("Render", ["Clouds", "Julia Fractal", "Mandelbrot Fractal", "Turbulence"]),
        ("Stylize", ["Edge Detect", "Emboss", "Outline", "Relief"]),
    ]

    static func adjustments(_ effects: [Engine.EffectInfo]) -> [Entry] {
        let registered = effects.filter { $0.category == "Adjustments" }
        var out: [Entry] = registered.map { e in
            let pdn = paintNetAdjustments.first { $0.0 == e.label }
            return .item(e.label, pdn?.1 ?? "", pdn?.2 ?? [], .effect(e.id))
        }
        let missing = paintNetAdjustments.filter { a in !registered.contains { $0.label == a.0 } }
        if !missing.isEmpty {
            out.append(.separator)
            out += missing.map { .item($0.0, $0.1, $0.2, .none) }
        }
        return out
    }

    /// The engine effect behind each paint.net Adjustments and Effects title
    /// (the catalog's `command`). Straighten has no engine effect yet.
    static let effectIDs: [String: String] = {
        var m: [String: String] = [:]
        for (title, _, _) in paintNetAdjustments {
            m[title] = "effect.adjust." + title.lowercased()
                .replacingOccurrences(of: " / ", with: "_")
                .replacingOccurrences(of: "-", with: "_").replacingOccurrences(of: " ", with: "_")
        }
        m["Temperature and Tint"] = "effect.adjust.temperature_tint"
        let ids: [String: String] = [
            "Ink Sketch": "artistic.ink_sketch", "Oil Painting": "artistic.oil_painting",
            "Pencil Sketch": "artistic.pencil_sketch", "Bokeh Blur": "blur.bokeh", "Fragment Blur": "blur.fragment",
            "Gaussian Blur": "blur.gaussian", "Median Blur": "blur.median", "Motion Blur": "blur.motion",
            "Radial Blur": "blur.radial", "Sketch Blur": "blur.sketch", "Square Blur": "blur.square",
            "Surface Blur": "blur.surface", "Unfocus": "blur.unfocus", "Zoom Blur": "blur.zoom",
            "Quantize": "color.quantize", "Bulge": "distort.bulge", "Crystalize": "distort.crystalize",
            "Dents": "distort.dents", "Frosted Glass": "distort.frosted_glass", "Morphology": "distort.morphology",
            "Pixelate": "distort.pixelate", "Polar Inversion": "distort.polar_inversion",
            "Tile Reflection": "distort.tile_reflection", "Twist": "distort.twist", "Add Noise": "noise.add",
            "Reduce Noise": "noise.reduce", "Drop Shadow": "object.drop_shadow", "Glow": "photo.glow",
            "Red Eye Removal": "photo.red_eye_removal", "Sharpen": "photo.sharpen",
            "Soften Portrait": "photo.soften_portrait", "Vignette": "photo.vignette", "Clouds": "render.clouds",
            "Julia Fractal": "render.julia_fractal", "Mandelbrot Fractal": "render.mandelbrot_fractal",
            "Turbulence": "render.turbulence", "Edge Detect": "stylize.edge_detect", "Emboss": "stylize.emboss",
            "Outline": "stylize.outline", "Relief": "stylize.relief",
        ]
        for (t, id) in ids { m[t] = "effect." + id }
        return m
    }()

    static func effectsMenu(_ effects: [Engine.EffectInfo]) -> [Entry] {
        var out: [Entry] = [
            .item("Repeat Effect", "f", cmd, .selector(#selector(EffectMenuActions.repeatEffect(_:)))), .separator,
        ]
        // Every category other than Adjustments and Edit names an Effects submenu
        // ("Blurs"); an "Effects/<Submenu>" form is accepted too.
        func submenu(_ e: Engine.EffectInfo) -> String {
            e.category.hasPrefix("Effects/") ? String(e.category.dropFirst("Effects/".count)) : e.category
        }
        let registered = effects.filter { !["Adjustments", "Edit", "Layers"].contains($0.category) }
        // paint.net's items, in its order, take the registered effect with the
        // same name wherever the engine files it (Median Blur's engine category
        // is Noise; paint.net 5.1 lists it under Blurs). Items with no effect
        // stay present and off.
        var placed = Set<String>()
        var slots: [String: [Entry]] = [:]
        for (name, titles) in paintNetEffects {
            slots[name] = titles.map { title in
                if let e = registered.first(where: { $0.label == title && !placed.contains($0.id) }) {
                    placed.insert(e.id)
                    return .item(title, "", [], .effect(e.id))
                }
                return i(title)
            }
        }
        var names = paintNetEffects.map(\.0)
        for e in registered where !placed.contains(e.id) && !names.contains(submenu(e)) {
            names.append(submenu(e))
        }
        for name in names {
            var entries = slots[name] ?? []
            // Engine effects paint.net does not list come last.
            let more = registered.filter { !placed.contains($0.id) && submenu($0) == name }
            if !entries.isEmpty, !more.isEmpty { entries.append(.separator) }
            entries += more.map { .item($0.label, "", [], .effect($0.id)) }
            out.append(.submenu(name, entries))
        }
        return out
    }

    static func entries(effects: [Engine.EffectInfo]) -> [Entry] {
        let editEffects: [Entry] = effects.filter { $0.category == "Edit" }.map { .item($0.label, "", [], .effect($0.id)) }
        // Layer transforms the engine registers as effects (category "Layers") fill
        // their Layers-menu item; until then the item stays a disabled placeholder.
        func layerItem(_ title: String) -> Entry {
            if let e = effects.first(where: { $0.category == "Layers" && $0.label == title }) {
                return .item(title, "", [], .effect(e.id))
            }
            return i(title)
        }
        return [
            .submenu("OpenPainter", [
                .item("About OpenPainter", "", [], .selector(#selector(NSApplication.orderFrontStandardAboutPanel(_:)))),
                .separator,
                .item("Settings…", ",", cmd, .selector(#selector(SettingsMenuActions.showSettings(_:)))),
                .separator,
                .item("Hide OpenPainter", "h", cmd, .selector(#selector(NSApplication.hide(_:)))),
                .item("Hide Others", "h", optCmd, .selector(#selector(NSApplication.hideOtherApplications(_:)))),
                .item("Show All", "", [], .selector(#selector(NSApplication.unhideAllApplications(_:)))),
                .separator,
                .item("Quit OpenPainter", "q", cmd, .selector(#selector(NSApplication.terminate(_:)))),
            ]),
            .submenu("File", [
                .item("New…", "n", cmd, .selector(#selector(NSDocumentController.newDocument(_:)))),  // M19
                .item("Open…", "o", cmd, .selector(#selector(NSDocumentController.openDocument(_:)))),
                .submenu("Open Recent", [
                    .item("Clear Menu", "", [], .selector(#selector(NSDocumentController.clearRecentDocuments(_:)))),
                ]),
                .submenu("Acquire", [i("From Scanner or Camera…")]),
                .separator,
                .item("Save", "s", cmd, .selector(#selector(NSDocument.save(_:)))),  // M19
                .item("Save As…", "s", shiftCmd, .selector(#selector(NSDocument.saveAs(_:)))),
                .item("Save a Copy…", "", [], .selector(#selector(NSDocument.saveTo(_:)))),  // M19
                // M55: off when no image has unsaved changes (MenuStates.swift).
                .item("Save All", "s", optCmd, .selector(#selector(NSDocumentController.saveAllDocuments(_:)))),
                .item("Revert to Saved", "", [], .selector(#selector(NSDocument.revertToSaved(_:)))),  // M19
                .separator,
                // Print is not in v1 (docs/PLAN.md, "Not in this version").
                .item("Close", "w", cmd, .selector(#selector(NSWindow.performClose(_:)))),
            ]),
            .submenu("Edit", [
                .item("Undo", "z", cmd, .selector(#selector(StandardEditActions.undo(_:)))),
                .item("Redo", "z", shiftCmd, .selector(#selector(StandardEditActions.redo(_:)))),
                .separator,
                i("Cut", "x"),
                i("Copy", "c"),
                i("Copy Merged", "c", shiftCmd),
                i("Paste", "v"),
                i("Paste into New Layer", "v", shiftCmd),
                i("Paste into New Image", "v", optCmd),
                .separator,
                // M29: SelectionEditActions.swift.
                .item("Copy Selection", "c", optShiftCmd, .selector(#selector(SelectionEditActions.copySelection(_:)))),
                .submenu("Paste Selection", SelectionClipboard.pasteTitles.enumerated().map { n, title in
                    .item(title, n == 0 ? "v" : "", n == 0 ? optShiftCmd : [],
                          .selector(#selector(SelectionEditActions.pasteSelection(_:))))
                }),
                .separator,
                .item("Erase Selection", deleteKey, [], .selector(#selector(SelectionEditActions.eraseSelection(_:)))),
                .item("Fill Selection", deleteKey, [.shift], .selector(#selector(SelectionEditActions.fillSelection(_:)))),
                .item("Invert Selection", "i", cmd, .command("selection.invert")),
                .item("Expand Selection…", "", [], .selector(#selector(SelectionEditActions.showExpandSelection(_:)))),
                .item("Select All", "a", cmd, .selector(#selector(NSResponder.selectAll(_:)))),
                .item("Deselect", "d", cmd, .command("selection.deselect")),
            ] + (editEffects.isEmpty ? [] : [.separator] + editEffects)),
            .submenu("View", [
                .item("Zoom In", "=", cmd, .selector(#selector(CanvasMenuActions.zoomIn(_:)))),
                .item("Zoom Out", "-", cmd, .selector(#selector(CanvasMenuActions.zoomOut(_:)))),
                // paint.net 5.1: Ctrl+B, Ctrl+Shift+B, Ctrl+0 (KeyboardMouseCommands.html).
                .item("Zoom to Window", "b", cmd, .selector(#selector(CanvasMenuActions.zoomToWindow(_:)))),
                .item("Zoom to Selection", "b", shiftCmd, .selector(#selector(EditorMenuActions.zoomToSelection(_:)))),
                .item("Actual Size", "0", cmd, .selector(#selector(CanvasMenuActions.actualSize(_:)))),
                .submenu("Zoom Presets", zoomPresetEntries),  // M56: paint.net's zoom presets
                .separator,
                .item("Pixel Grid", "'", cmd, .selector(#selector(CanvasMenuActions.togglePixelGrid(_:)))),
                .item("Rulers", "r", optCmd, .selector(#selector(ViewMenuActions.toggleRulers(_:)))),
                .separator,
                .item("Pixels", "", [], .selector(#selector(EditorMenuActions.setUnits(_:)))),
                .item("Inches", "", [], .selector(#selector(EditorMenuActions.setUnits(_:)))),
                .item("Centimeters", "", [], .selector(#selector(EditorMenuActions.setUnits(_:)))),
            ]),
            .submenu("Image", [
                .item("Crop to Selection", "x", shiftCmd, .command("image.crop_to_selection")),
                .item("Crop Tool", "", [], .selector(#selector(ImageSizeActions.showCropTool(_:)))),
                .item("Resize…", "r", cmd, .selector(#selector(ImageSizeActions.showResizeImage(_:)))),
                .item("Canvas Size…", "r", shiftCmd, .selector(#selector(ImageSizeActions.showCanvasSize(_:)))),
                .separator,
                .item("Flip Horizontal", "", [], .command("image.flip_horizontal")),
                .item("Flip Vertical", "", [], .command("image.flip_vertical")),
                .separator,
                .item("Rotate 90° Clockwise", "", [], .command("image.rotate_90_cw")),
                .item("Rotate 90° Counter-Clockwise", "g", cmd, .command("image.rotate_90_ccw")),
                .item("Rotate 180°", "j", cmd, .command("image.rotate_180")),
                .separator,
                .item("Color Profile…", "", [], .selector(#selector(ColorProfileMenuActions.showColorProfile(_:)))),
                .separator,
                .item("Flatten", "f", shiftCmd, .command("image.flatten")),
            ]),
            .submenu("Layers", [
                .item("Add New Layer", "n", shiftCmd, .selector(#selector(EditorMenuActions.addLayer(_:)))),
                .item("Delete Layer", deleteKey, shiftCmd, .command("layer.delete")),
                .item("Duplicate Layer", "d", shiftCmd, .command("layer.duplicate")),
                .item("Merge Layer Down", "", [], .command("layer.merge_down")),
                .item("Toggle Layer Visibility", "", [], .selector(#selector(LayerMenuActions.toggleLayerVisibility(_:)))),
                // M55: the M49 engine commands (MenuStates.swift has the actions).
                .item("Import From File…", "", [], .selector(#selector(LayerEdgeMenuActions.importLayerFromFile(_:)))),
                .separator,
                .item("Flip Horizontal", "", [], .command("layer.flip_horizontal")),
                .item("Flip Vertical", "", [], .command("layer.flip_vertical")),
                .item("Rotate 180°", "", [], .command("layer.rotate_180")),
                layerItem("Rotate / Zoom…"),
                .separator,
                // paint.net: Ctrl+Alt+PgUp, Alt+PgUp, Alt+PgDn, Ctrl+Alt+PgDn.
                .item("Go to Top Layer", key(NSPageUpFunctionKey), optCmd, .selector(#selector(EditorMenuActions.goToLayer(_:)))),
                .item("Go to Layer Above", key(NSPageUpFunctionKey), opt, .selector(#selector(EditorMenuActions.goToLayer(_:)))),
                .item("Go to Layer Below", key(NSPageDownFunctionKey), opt, .selector(#selector(EditorMenuActions.goToLayer(_:)))),
                .item("Go to Bottom Layer", key(NSPageDownFunctionKey), optCmd, .selector(#selector(EditorMenuActions.goToLayer(_:)))),
                .separator,
                .item("Move Layer to Top", "", [], .selector(#selector(LayerEdgeMenuActions.moveLayerToTop(_:)))),
                .item("Move Layer Up", "", [], .command("layer.move_up")),
                .item("Move Layer Down", "", [], .command("layer.move_down")),
                .item("Move Layer to Bottom", "", [], .selector(#selector(LayerEdgeMenuActions.moveLayerToBottom(_:)))),
                .separator,
                .item("Layer Properties…", key(NSF4FunctionKey), [], .selector(#selector(LayerMenuActions.showLayerProperties(_:)))),
            ]),
            .submenu("Adjustments", adjustments(effects)),
            .submenu("Effects", effectsMenu(effects)),
            .submenu("Window", PanelKind.allCases.map { .item($0.title, key(NSF5FunctionKey - 5 + $0.functionKey), [], .panel($0)) } + [
                .separator,
                .item("Reset Panel Layout", "", [], .resetPanels),
                .separator,
                .item("Minimize", "m", cmd, .selector(#selector(NSWindow.performMiniaturize(_:)))),
                .item("Zoom", "", [], .selector(#selector(NSWindow.performZoom(_:)))),
                .separator,
                .item("Bring All to Front", "", [], .selector(#selector(NSApplication.arrangeInFront(_:)))),
            ]),
            // paint.net's Help menu (HelpMenu.html) links to its web pages;
            // OpenPainter's pages do not exist yet, so these stay off. Plugins
            // is not in v1. Check for Updates (M58) uses Sparkle (Updates.swift).
            .submenu("Help", [
                i("OpenPainter Help", "?"),
                .separator,
                .item("Check for Updates…", "", [], .selector(#selector(UpdateMenuActions.checkForUpdates(_:)))),
                i("Send Feedback…"),
            ]),
        ]
    }

    /// Builds the menu bar. `panelTarget` receives the Window menu's panel items.
    @MainActor
    public static func build(effects: [Engine.EffectInfo], panelTarget: AnyObject?) -> NSMenu {
        let main = NSMenu(title: "Main")
        for case let .submenu(title, items) in entries(effects: effects) {
            let menu = NSMenu(title: title)
            fill(menu, items, panelTarget: panelTarget, path: title)
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.submenu = menu
            main.addItem(item)
        }
        return main
    }

    /// The tag of a Layers > Go to … item: 0 top, 1 above, 2 below, 3 bottom.
    static let goToTitles = ["Go to Top Layer", "Go to Layer Above", "Go to Layer Below", "Go to Bottom Layer"]

    @MainActor
    static func fill(_ menu: NSMenu, _ entries: [Entry], panelTarget: AnyObject?, path: String = "") {
        for entry in entries {
            switch entry {
            case .separator:
                menu.addItem(.separator())
            case let .submenu(title, items):
                let sub = NSMenu(title: title)
                fill(sub, items, panelTarget: panelTarget, path: "\(path) > \(title)")
                let item = menu.addItem(withTitle: title, action: nil, keyEquivalent: "")
                item.submenu = sub
            case let .item(title, key, mods, act):
                let item = NSMenuItem(title: title, action: nil, keyEquivalent: key)
                // M55: an SF Symbol where paint.net shows an icon (MenuIcons.swift).
                item.image = MenuIcons.image(for: "\(path) > \(title)")
                item.keyEquivalentModifierMask = mods
                switch act {
                case let .selector(s):
                    item.action = s
                    if let tag = goToTitles.firstIndex(of: title) { item.tag = tag }
                    if s == #selector(SelectionEditActions.pasteSelection(_:)),
                       let tag = SelectionClipboard.pasteTitles.firstIndex(of: title) { item.tag = tag }
                    // Settings… works with no document window (M56).
                    if s == #selector(SettingsMenuActions.showSettings(_:)) { item.target = SettingsMenuTarget.shared }
                    // Check for Updates… works with no document window (M58).
                    if s == #selector(UpdateMenuActions.checkForUpdates(_:)) { item.target = UpdateMenuTarget.shared }
                case let .command(id):
                    item.action = #selector(EditorMenuActions.runCommand(_:))
                    item.representedObject = id
                case let .effect(id):
                    item.action = #selector(EditorMenuActions.runEffect(_:))
                    item.representedObject = id
                    // The menu opens the effect's dialog (M24, EffectDialog.swift).
                    item.target = EffectMenuTarget.shared
                case let .panel(kind):
                    item.action = #selector(PanelMenuTarget.togglePanel(_:))
                    item.target = panelTarget
                    item.tag = kind.digit
                    item.toolTip = "\(kind.title) (F\(kind.functionKey) or ⌥⌘\(kind.digit))"
                case .resetPanels:
                    item.action = #selector(PanelMenuTarget.resetPanelLayout(_:))
                    item.target = panelTarget
                case .none:
                    item.toolTip = "Comes in a later version"
                }
                menu.addItem(item)
                // The second panel shortcut: a hidden twin with Option-Command-digit.
                if case let .panel(kind) = act {
                    let twin = NSMenuItem(
                        title: "\(title) (\(kind.digit))", action: #selector(PanelMenuTarget.togglePanel(_:)),
                        keyEquivalent: "\(kind.digit)")
                    twin.keyEquivalentModifierMask = optCmd
                    twin.target = panelTarget
                    twin.tag = kind.digit
                    twin.isHidden = true
                    twin.allowsKeyEquivalentWhenHidden = true
                    menu.addItem(twin)
                }
            }
        }
    }

    /// One line per visible item, for the checked-in skeleton list:
    /// "Menu > Submenu > Item [shortcut] (off)". Separators are "Menu > ---".
    @MainActor
    public static func describe(_ main: NSMenu) -> [String] {
        var lines: [String] = []
        func walk(_ menu: NSMenu, _ path: String) {
            for item in menu.items where !item.isHidden {
                if item.isSeparatorItem {
                    lines.append("\(path) > ---")
                    continue
                }
                let here = path.isEmpty ? item.title : "\(path) > \(item.title)"
                if let sub = item.submenu {
                    lines.append(here)
                    walk(sub, here)
                } else {
                    var line = here
                    if !item.keyEquivalent.isEmpty { line += " [\(shortcut(item))]" }
                    if item.action == nil { line += " (off)" }
                    lines.append(line)
                }
            }
        }
        walk(main, "")
        return lines
    }

    static func shortcut(_ item: NSMenuItem) -> String {
        var s = ""
        let m = item.keyEquivalentModifierMask
        if m.contains(.control) { s += "⌃" }
        if m.contains(.option) { s += "⌥" }
        if m.contains(.shift) { s += "⇧" }
        if m.contains(.command) { s += "⌘" }
        let k = item.keyEquivalent
        let names: [Int: String] = [
            8: "⌫", NSF4FunctionKey: "F4", NSF5FunctionKey: "F5", NSF6FunctionKey: "F6", NSF7FunctionKey: "F7",
            NSF8FunctionKey: "F8", NSHomeFunctionKey: "Home", NSEndFunctionKey: "End",
            NSPageUpFunctionKey: "PageUp", NSPageDownFunctionKey: "PageDown",
        ]
        if let scalar = k.unicodeScalars.first, let name = names[Int(scalar.value)] { return s + name }
        return s + k.uppercased()
    }
}

/// Receives the Window menu's panel items and keeps their check marks equal to
/// the panels' shown state.
@MainActor
public final class PanelMenuTarget: NSObject, NSMenuItemValidation {
    weak var panels: PanelManager?

    public init(panels: PanelManager) {
        self.panels = panels
    }

    static func kind(_ tag: Int) -> PanelKind? {
        tag >= 1 && tag <= PanelKind.allCases.count ? PanelKind.allCases[tag - 1] : nil
    }

    @objc public func togglePanel(_ sender: Any?) {
        guard let tag = (sender as? NSMenuItem)?.tag ?? (sender as? NSControl)?.tag, let k = Self.kind(tag) else { return }
        panels?.toggle(k)
    }

    @objc public func resetPanelLayout(_ sender: Any?) {
        panels?.resetLayout()
    }

    public func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(togglePanel(_:)), let k = Self.kind(item.tag) {
            item.state = panels?.isShown(k) == true ? .on : .off
        }
        return panels != nil
    }
}
