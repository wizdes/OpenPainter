// Menu icons (M55): an SF Symbol for every menu item that paint.net 5.1 shows
// an icon for (tests/catalog/paintnet-5.1-menus.toml, `icon = true`). Each
// symbol is chosen for the item's meaning; no paint.net artwork is used.
// Keys are the item's path in the menu bar, as MenuSkeleton.describe writes it.
import AppKit

public enum MenuIcons {
    /// Menu path to SF Symbol name.
    public static let symbols: [String: String] = {
        var s: [String: String] = [
            // Application menu (paint.net's Help > About, Settings icon, File > Exit).
            "OpenPainter > About OpenPainter": "info.circle",
            "OpenPainter > Settings…": "gearshape",
            "OpenPainter > Quit OpenPainter": "power",
            // File
            "File > New…": "doc.badge.plus",
            "File > Open…": "folder",
            "File > Open Recent > Clear Menu": "xmark",
            "File > Save": "square.and.arrow.down",
            "File > Save As…": "square.and.arrow.down.on.square",
            "File > Save All": "tray.and.arrow.down",
            "File > Close": "xmark.square",
            // Edit
            "Edit > Undo": "arrow.uturn.backward",
            "Edit > Redo": "arrow.uturn.forward",
            "Edit > Cut": "scissors",
            "Edit > Copy": "doc.on.doc",
            "Edit > Copy Merged": "square.3.layers.3d",
            "Edit > Paste": "doc.on.clipboard",
            "Edit > Paste into New Layer": "plus.rectangle.on.rectangle",
            "Edit > Paste into New Image": "photo.badge.plus",
            "Edit > Copy Selection": "rectangle.dashed.and.paperclip",
            "Edit > Paste Selection > Replace": "rectangle.dashed",
            "Edit > Paste Selection > Add (union)": "plus.square.dashed",
            "Edit > Paste Selection > Exclude": "minus.square",
            "Edit > Paste Selection > Intersect": "square.on.square.intersection.dashed",
            "Edit > Paste Selection > Invert (\"xor\")": "square.on.square.squareshape.controlhandles",
            "Edit > Erase Selection": "eraser",
            "Edit > Fill Selection": "drop.fill",
            "Edit > Invert Selection": "square.on.square.dashed",
            "Edit > Select All": "selection.pin.in.out",
            "Edit > Deselect": "rectangle.dashed.badge.record",
            // View
            "View > Zoom In": "plus.magnifyingglass",
            "View > Zoom Out": "minus.magnifyingglass",
            "View > Zoom to Window": "arrow.up.left.and.down.right.magnifyingglass",
            "View > Zoom to Selection": "viewfinder",
            "View > Actual Size": "1.magnifyingglass",
            "View > Pixel Grid": "grid",
            "View > Rulers": "ruler",
            // Image
            "Image > Crop to Selection": "crop",
            "Image > Resize…": "arrow.up.left.and.arrow.down.right",
            "Image > Canvas Size…": "square.dashed.inset.filled",
            "Image > Flip Horizontal": "arrow.left.and.right.righttriangle.left.righttriangle.right",
            "Image > Flip Vertical": "arrow.up.and.down.righttriangle.up.righttriangle.down",
            "Image > Rotate 90° Clockwise": "rotate.right",
            "Image > Rotate 90° Counter-Clockwise": "rotate.left",
            "Image > Rotate 180°": "arrow.triangle.2.circlepath",
            "Image > Color Profile…": "camera.filters",
            "Image > Flatten": "square.3.layers.3d.down.right",
            // Layers
            "Layers > Add New Layer": "plus.square.on.square",
            "Layers > Delete Layer": "trash",
            "Layers > Duplicate Layer": "square.on.square",
            "Layers > Merge Layer Down": "arrow.down.square",
            "Layers > Toggle Layer Visibility": "eye",
            "Layers > Import From File…": "photo.on.rectangle",
            "Layers > Flip Horizontal": "arrow.left.and.right.square",
            "Layers > Flip Vertical": "arrow.up.and.down.square",
            "Layers > Rotate 180°": "arrow.clockwise.square",
            "Layers > Rotate / Zoom…": "rotate.3d",
            "Layers > Go to Top Layer": "arrow.up.to.line",
            "Layers > Go to Layer Above": "arrow.up",
            "Layers > Go to Layer Below": "arrow.down",
            "Layers > Go to Bottom Layer": "arrow.down.to.line",
            "Layers > Move Layer to Top": "chevron.up.2",
            "Layers > Move Layer Up": "chevron.up",
            "Layers > Move Layer Down": "chevron.down",
            "Layers > Move Layer to Bottom": "chevron.down.2",
            "Layers > Layer Properties…": "slider.horizontal.3",
            // Effects > Repeat shows the last effect's icon once one has run.
            "Effects > Repeat Effect": "arrow.clockwise",
            // Help
            "Help > OpenPainter Help": "questionmark.circle",
            "Help > Send Feedback…": "envelope",
        ]
        for (id, symbol) in effectSymbols {
            if let path = effectPaths[id] { s[path] = symbol }
        }
        return s
    }()

    /// Effect and adjustment id to SF Symbol name.
    public static let effectSymbols: [String: String] = [
        "effect.adjust.auto_level": "wand.and.stars",
        "effect.adjust.black_and_white": "circle.lefthalf.filled",
        "effect.adjust.brightness_contrast": "sun.max",
        "effect.adjust.curves": "point.topleft.down.to.point.bottomright.curvepath",
        "effect.adjust.exposure": "plusminus.circle",
        "effect.adjust.highlights_shadows": "circle.bottomhalf.filled",
        "effect.adjust.hue_saturation": "paintpalette",
        "effect.adjust.invert_alpha": "checkerboard.rectangle",
        "effect.adjust.invert_colors": "circle.righthalf.filled",
        "effect.adjust.levels": "chart.bar",
        "effect.adjust.posterize": "square.split.2x2",
        "effect.adjust.sepia": "photo",
        "effect.adjust.temperature_tint": "thermometer.medium",
        "effect.artistic.ink_sketch": "pencil.tip",
        "effect.artistic.oil_painting": "paintbrush",
        "effect.artistic.pencil_sketch": "pencil",
        "effect.blur.bokeh": "circle.hexagongrid",
        "effect.blur.fragment": "square.grid.2x2",
        "effect.blur.gaussian": "drop",
        "effect.blur.median": "line.3.horizontal.decrease.circle",
        "effect.blur.motion": "wind",
        "effect.blur.radial": "circle.circle",
        "effect.blur.sketch": "scribble",
        "effect.blur.square": "square.dashed",
        "effect.blur.surface": "water.waves",
        "effect.blur.unfocus": "camera.aperture",
        "effect.blur.zoom": "arrow.up.left.and.arrow.down.right.circle",
        "effect.color.quantize": "square.grid.3x3",
        "effect.distort.bulge": "smallcircle.filled.circle",
        "effect.distort.crystalize": "diamond",
        "effect.distort.dents": "hurricane",
        "effect.distort.frosted_glass": "snowflake",
        "effect.distort.morphology": "circle.grid.cross",
        "effect.distort.pixelate": "squareshape.split.3x3",
        "effect.distort.polar_inversion": "arrow.triangle.2.circlepath.circle",
        "effect.distort.tile_reflection": "square.grid.3x3.topleft.filled",
        "effect.distort.twist": "tornado",
        "effect.noise.add": "speaker.wave.2",
        "effect.noise.reduce": "speaker.slash",
        "effect.object.drop_shadow": "shadow",
        "effect.photo.glow": "lightbulb",
        "effect.photo.red_eye_removal": "eye.circle",
        "effect.photo.sharpen": "triangle",
        "effect.photo.soften_portrait": "person.crop.square",
        "effect.photo.vignette": "circle.dashed.inset.filled",
        "effect.render.clouds": "cloud",
        "effect.render.julia_fractal": "sparkles",
        "effect.render.mandelbrot_fractal": "aqi.medium",
        "effect.render.turbulence": "dot.radiowaves.left.and.right",
        "effect.stylize.edge_detect": "squareshape.dashed.squareshape",
        "effect.stylize.emboss": "cube",
        "effect.stylize.outline": "circle.dashed",
        "effect.stylize.relief": "mountain.2",
        "effect.layers.rotate_zoom": "rotate.3d",
    ]

    /// Straighten is not built yet (pending); its off item still shows an icon.
    static let placeholderSymbols: [String: String] = [
        "Effects > Photo > Straighten": "level",
    ]

    /// Where each effect sits in the menu bar.
    static var effectPaths: [String: String] {
        var p: [String: String] = [:]
        for (title, _, _) in MenuSkeleton.paintNetAdjustments {
            if let id = MenuSkeleton.effectIDs[title] { p[id] = "Adjustments > \(title)" }
        }
        for (sub, titles) in MenuSkeleton.paintNetEffects {
            for t in titles {
                if let id = MenuSkeleton.effectIDs[t] { p[id] = "Effects > \(sub) > \(t)" }
            }
        }
        p["effect.layers.rotate_zoom"] = "Layers > Rotate / Zoom…"
        return p
    }

    /// The symbol for a menu path, if paint.net shows an icon there.
    public static func symbol(for path: String) -> String? {
        symbols[path] ?? placeholderSymbols[path]
    }

    public static func image(for path: String) -> NSImage? {
        symbol(for: path).flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }
    }

    /// The icon of an effect (Effects > Repeat shows the last one's).
    public static func effectImage(_ id: String) -> NSImage? {
        effectSymbols[id].flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }
    }
}
