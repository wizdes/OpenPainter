// Effect parameter schemas (M24). The engine's `effects.list` gives each effect
// a schema: a list of fields with a key, a kind (int, float, bool, string,
// {"choice": [...]}, curve, rgb_bytes, rgb_gamma), an optional min and max and
// a default. This file turns that JSON into Swift values, groups fields into
// the dialog's controls (angle dials, point pads, the Curves and Levels
// editors) and names each field in paint.net's wording.
import Foundation

/// One parameter value, in the shape the engine expects.
public enum ParamValue: Equatable, Sendable {
    case int(Int)
    case float(Double)
    case bool(Bool)
    case string(String)
    /// Curves: one list of [x, y] points (0...255) per channel (1 or 3).
    case curve([[[Int]]])
    /// Three (or four, for colors with alpha) bytes.
    case bytes([Int])
    /// Three gamma values, 0.1...10.
    case gamma([Double])

    /// The value as a JSON-ready object for `JSONSerialization`.
    public var json: any Sendable {
        switch self {
        case let .int(v): return v
        case let .float(v): return v
        case let .bool(v): return v
        case let .string(v): return v
        case let .curve(v): return v
        case let .bytes(v): return v
        case let .gamma(v): return v
        }
    }

    public var number: Double? {
        switch self {
        case let .int(v): return Double(v)
        case let .float(v): return v
        default: return nil
        }
    }
}

/// One schema field.
public struct EffectParam: Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        case int, float, bool, string, curve, rgbBytes, rgbGamma
        case choice([String])
    }

    public let key: String
    public let kind: Kind
    public let min: Double?
    public let max: Double?
    public let defaultValue: ParamValue

    public init(key: String, kind: Kind, min: Double?, max: Double?, defaultValue: ParamValue) {
        self.key = key
        self.kind = kind
        self.min = min
        self.max = max
        self.defaultValue = defaultValue
    }

    /// Parses one `schema` entry. Throws when the kind or default is not usable.
    init(json: [String: Any]) throws {
        guard let key = json["key"] as? String else { throw EffectSchemaError.bad("a field has no key") }
        self.key = key
        min = (json["min"] as? NSNumber)?.doubleValue
        max = (json["max"] as? NSNumber)?.doubleValue
        let d = json["default"]
        if let name = json["kind"] as? String {
            switch name {
            case "int": kind = .int
            case "float": kind = .float
            case "bool": kind = .bool
            case "string": kind = .string
            case "curve": kind = .curve
            case "rgb_bytes": kind = .rgbBytes
            case "rgb_gamma": kind = .rgbGamma
            default: throw EffectSchemaError.bad("\(key): unknown kind \(name)")
            }
        } else if let obj = json["kind"] as? [String: Any], let choices = obj["choice"] as? [String], !choices.isEmpty {
            kind = .choice(choices)
        } else {
            throw EffectSchemaError.bad("\(key): unknown kind")
        }
        defaultValue = try Self.value(d, kind: kind, key: key, min: min)
    }

    /// Reads a JSON value as this kind. A missing default falls back to the
    /// minimum (numbers), false, "", the first choice, or an identity curve.
    static func value(_ d: Any?, kind: Kind, key: String, min: Double?) throws -> ParamValue {
        switch kind {
        case .int: return .int((d as? NSNumber)?.intValue ?? Int(min ?? 0))
        case .float: return .float((d as? NSNumber)?.doubleValue ?? (min ?? 0))
        case .bool: return .bool((d as? NSNumber)?.boolValue ?? false)
        case .string: return .string(d as? String ?? "")
        case let .choice(c):
            let s = d as? String ?? c[0]
            guard c.contains(s) else { throw EffectSchemaError.bad("\(key): default \(s) is not a choice") }
            return .string(s)
        case .curve:
            guard let channels = d as? [[[NSNumber]]] else { return .curve([[[0, 0], [255, 255]]]) }
            return .curve(channels.map { $0.map { $0.map(\.intValue) } })
        case .rgbBytes:
            guard let b = d as? [NSNumber] else { throw EffectSchemaError.bad("\(key): no default bytes") }
            return .bytes(b.map(\.intValue))
        case .rgbGamma:
            guard let g = d as? [NSNumber] else { return .gamma([1, 1, 1]) }
            return .gamma(g.map(\.doubleValue))
        }
    }

    /// Slider step: 1 for integers; for floats, by range (0.01 up to a range of
    /// 10, 0.1 up to 1000, else 1).
    public var step: Double {
        if kind == .int { return 1 }
        guard let min, let max else { return 0.01 }
        let range = max - min
        return range <= 10 ? 0.01 : range <= 1000 ? 0.1 : 1
    }

    /// Digits after the point in the number field.
    public var decimals: Int {
        let s = step
        return s >= 1 ? 0 : s >= 0.1 ? 1 : 2
    }

    /// True for a random seed (shown as a number field and a Reseed button).
    public var isSeed: Bool { kind == .int && ["seed", "randomize"].contains(key) }

    /// Clamps a number to the schema range and rounds it to the kind.
    public func clamp(_ v: Double) -> ParamValue {
        var x = v
        if let min { x = Swift.max(x, min) }
        if let max { x = Swift.min(x, max) }
        return kind == .int ? .int(Int(x.rounded())) : .float(x)
    }
}

public enum EffectSchemaError: Error, Equatable {
    case bad(String)
}

/// One control in an effect dialog. Most fields get one control each; some
/// pairs or sets of fields share a control.
public enum EffectControl: Equatable, Sendable {
    /// Int or Float: slider and number field (seed: field and Reseed).
    case number(EffectParam)
    case angle(EffectParam)
    /// Two Floats (x, y), such as center_x / center_y, picked on a thumbnail.
    case point(x: EffectParam, y: EffectParam, label: String)
    case check(EffectParam)
    case popup(EffectParam)
    case text(EffectParam)
    /// A color (rgb_bytes outside Levels), with an optional alpha field.
    case color(EffectParam)
    /// Curves: the points field plus its transfer-mode choice (when present).
    case curves(points: EffectParam, mode: EffectParam?)
    /// Levels: input black/white, gamma and output black/white.
    case levels(inputBlack: EffectParam, inputWhite: EffectParam, gamma: EffectParam,
                outputBlack: EffectParam, outputWhite: EffectParam)

    /// The schema keys this control edits.
    public var keys: [String] {
        switch self {
        case let .number(p), let .angle(p), let .check(p), let .popup(p), let .text(p), let .color(p): return [p.key]
        case let .point(x, y, _): return [x.key, y.key]
        case let .curves(points, mode): return [points.key] + (mode.map { [$0.key] } ?? [])
        case let .levels(a, b, c, d, e): return [a.key, b.key, c.key, d.key, e.key]
        }
    }
}

/// An effect from the catalog with its parsed schema.
public struct EffectSchema: Equatable, Sendable {
    public let id: String
    public let label: String
    public let category: String
    public let params: [EffectParam]

    public init(id: String, label: String, category: String, params: [EffectParam]) {
        self.id = id
        self.label = label
        self.category = category
        self.params = params
    }

    init(json: [String: Any]) throws {
        guard let id = json["id"] as? String else { throw EffectSchemaError.bad("an effect has no id") }
        self.id = id
        label = json["label"] as? String ?? id
        category = json["category"] as? String ?? ""
        let fields = json["schema"] as? [[String: Any]] ?? []
        params = try fields.map { f in
            do { return try EffectParam(json: f) } catch let EffectSchemaError.bad(why) {
                throw EffectSchemaError.bad("\(id): \(why)")
            }
        }
    }

    public var hasParameters: Bool { !params.isEmpty }

    /// True when the effect takes a random seed (shows Reseed).
    public var isSeeded: Bool { params.contains { $0.isSeed } }

    public var defaults: [String: ParamValue] {
        Dictionary(uniqueKeysWithValues: params.map { ($0.key, $0.defaultValue) })
    }

    public func param(_ key: String) -> EffectParam? { params.first { $0.key == key } }

    /// The dialog's controls in schema order. A group takes the place of its
    /// first field.
    public func controls() -> [EffectControl] {
        var used = Set<String>()
        var out: [EffectControl] = []
        let levelKeys = ["InputBlack", "InputWhite", "Gamma", "OutputBlack", "OutputWhite"]
        let levels = levelKeys.compactMap { param($0) }
        let isLevels = levels.count == 5 && levels[2].kind == .rgbGamma
            && [0, 1, 3, 4].allSatisfy { levels[$0].kind == .rgbBytes }
        for p in params where !used.contains(p.key) {
            if isLevels, levelKeys.contains(p.key) {
                out.append(.levels(inputBlack: levels[0], inputWhite: levels[1], gamma: levels[2],
                                   outputBlack: levels[3], outputWhite: levels[4]))
                used.formUnion(levelKeys)
                continue
            }
            switch p.kind {
            case .curve:
                let mode = params.first { if case .choice = $0.kind { return $0.key == "ColorTransferMode" }; return false }
                out.append(.curves(points: p, mode: mode))
                used.insert(p.key)
                if let mode { used.insert(mode.key) }
                continue
            case .choice where p.key == "ColorTransferMode" && params.contains { $0.kind == .curve }:
                // The Curves editor owns its mode popup; place the editor here.
                let points = params.first { $0.kind == .curve }!
                out.append(.curves(points: points, mode: p))
                used.formUnion([p.key, points.key])
                continue
            default: break
            }
            if let pair = Self.pointPair(p.key), p.kind == .float,
               let y = param(pair.y), y.kind == .float, !used.contains(y.key) {
                out.append(.point(x: p, y: y, label: EffectLabels.label(pair.name)))
                used.formUnion([p.key, y.key])
                continue
            }
            used.insert(p.key)
            switch p.kind {
            case .int, .float:
                out.append(Self.isAngle(p) ? .angle(p) : .number(p))
            case .bool: out.append(.check(p))
            case .choice: out.append(.popup(p))
            case .string: out.append(.text(p))
            case .rgbBytes: out.append(.color(p))
            case .rgbGamma: out.append(.number(p))  // not reached by any registered effect
            case .curve: break
            }
        }
        return out
    }

    /// Parameters named angle (or ending in _angle) get the dial. paint.net
    /// also shows Rotation as an angle.
    static func isAngle(_ p: EffectParam) -> Bool {
        guard p.kind == .float || p.kind == .int else { return false }
        return p.key == "angle" || p.key.hasSuffix("_angle") || p.key == "rotation"
    }

    /// `center_x` -> (center_y, "center"); also offset_ and pan_.
    static func pointPair(_ key: String) -> (y: String, name: String)? {
        for name in ["center", "offset", "pan"] where key == "\(name)_x" {
            return ("\(name)_y", name)
        }
        return nil
    }
}

/// The engine's effect catalog with schemas, read once per engine.
@MainActor
public enum EffectCatalog {
    private static var cache: [ObjectIdentifier: [String: EffectSchema]] = [:]

    /// Every registered effect's schema. Throws if the catalog cannot be read
    /// or a schema cannot be parsed.
    public static func all(_ engine: Engine) throws -> [EffectSchema] {
        try engine.effectCatalog().map(EffectSchema.init(json:))
    }

    public static func schema(_ id: String, engine: Engine) throws -> EffectSchema? {
        let key = ObjectIdentifier(engine)
        if cache[key] == nil {
            cache[key] = Dictionary(uniqueKeysWithValues: try all(engine).map { ($0.id, $0) })
        }
        return cache[key]?[id]
    }
}

/// Field labels in paint.net's wording. Keys not in the table are title-cased
/// ("cell_size" -> "Cell Size", "ColorTransferMode" -> "Color Transfer Mode").
public enum EffectLabels {
    /// paint.net 5.1 dialog wording, from its Adjustments and Effects pages
    /// (https://www.getpaint.net/doc/latest/AdjustmentsMenu.html and the Effects pages).
    static let table: [String: String] = [
        "radius": "Radius",
        "angle": "Angle",
        "amount": "Amount",
        "strength": "Strength",
        "quality": "Quality",
        "seed": "Random seed",
        "randomize": "Random seed",
        "brightness": "Brightness",
        "contrast": "Contrast",
        "saturation": "Saturation",
        "intensity": "Intensity",
        "coloring": "Coloring",
        "center": "Center",
        "offset": "Offset",
        "pan": "Pan",
        "zoom": "Zoom",
        "tilt": "Tilt",
        "scale": "Scale",
        "size": "Size",
        "cell_size": "Cell size",
        "tile_size": "Tile size",
        "brush_size": "Brush size",
        "pencil_tip_size": "Pencil tip size",
        "ink_outline": "Ink outline",
        "color_range": "Color range",
        "distance": "Distance",
        "fragments": "Fragments",
        "rotation": "Rotation",
        "centered": "Centered",
        "smoothness": "Smoothness",
        "roughness": "Roughness",
        "percentile": "Percentile",
        "threshold": "Threshold",
        "tolerance": "Tolerance",
        "thickness": "Thickness",
        "softness": "Softness",
        "warmth": "Warmth",
        "tension": "Tension",
        "refraction": "Refraction",
        "coverage": "Coverage",
        "coarseness": "Coarseness",
        "curvature": "Curvature",
        "factor": "Factor",
        "period": "Period",
        "octaves": "Octaves",
        "noise": "Noise",
        "linked": "Linked",
        "lighting": "Lighting",
        "opacity": "Opacity",
        "blend_mode": "Blend mode",
        "blend": "Blend mode",
        "invert_colors": "Invert colors",
        "overlay_edges": "Overlay edges",
        "only_draw_shadow": "Only draw shadow",
        "shadow_radius": "Shadow radius",
        "gamma_boost": "Gamma boost",
        "gamma": "Gamma",
        "gamma_exponent": "Gamma exponent",
        "linear_light": "Linear light",
        "dithering": "Dithering",
        "algorithm": "Algorithm",
        "colors": "Colors",
        "color": "Color",
        "color_1": "Primary color",
        "color_2": "Secondary color",
        "color_1_alpha": "Primary alpha",
        "color_2_alpha": "Secondary alpha",
        "diffusion": "Diffusion",
        "blurring": "Blurring",
        "axis": "Axis",
        "range": "Range",
        "width": "Width",
        "height": "Height",
        "ColorTransferMode": "Transfer map",
        "ControlPoints": "Curve",
        "InputBlack": "Input black",
        "InputWhite": "Input white",
        "OutputBlack": "Output black",
        "OutputWhite": "Output white",
        "Gamma": "Gamma",
        "Auto": "Auto",
    ]

    public static func label(_ key: String) -> String {
        table[key] ?? titleCase(key)
    }

    public static func titleCase(_ key: String) -> String {
        var words: [String] = []
        var word = ""
        for ch in key {
            if ch == "_" || ch == "-" || ch == " " {
                if !word.isEmpty { words.append(word) }
                word = ""
            } else if ch.isUppercase, let last = word.last, last.isLowercase {
                words.append(word)
                word = String(ch)
            } else {
                word.append(ch)
            }
        }
        if !word.isEmpty { words.append(word) }
        return words.map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
    }
}
