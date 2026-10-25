// Floating-panel geometry: default places, the clear rectangle the image fits
// into, no overlap between panels, and keeping panels on a visible screen.
// Pure math in AppKit screen coordinates (y grows up). See docs/design/SPEC.md,
// "Window and layout".
import Foundation

public enum PanelKind: String, CaseIterable, Sendable {
    // Toolbar and Window-menu order.
    case tools, history, layers, colors

    public var title: String { rawValue.capitalized }

    /// F5 to F8 (paint.net) and Option-Command-1 to 4.
    public var functionKey: Int { 5 + (Self.allCases.firstIndex(of: self) ?? 0) }
    public var digit: Int { 1 + (Self.allCases.firstIndex(of: self) ?? 0) }

    /// Default frame size in points (SPEC table).
    public var defaultSize: CGSize {
        switch self {
        case .tools: CGSize(width: 84, height: 292)
        case .colors: CGSize(width: 244, height: 246)
        case .history, .layers: CGSize(width: 226, height: 245)
        }
    }

    public var minimumSize: CGSize {
        switch self {
        case .tools: CGSize(width: 84, height: 94)
        case .colors: CGSize(width: 244, height: 246)
        case .history, .layers: CGSize(width: 226, height: 160)
        }
    }

    public var symbol: String {
        switch self {
        case .tools: "paintbrush.pointed"
        case .history: "clock.arrow.circlepath"
        case .layers: "square.3.layers.3d"
        case .colors: "paintpalette"
        }
    }
}

public enum PanelLayout {
    /// Space between the stage edge and a panel, and between panels.
    public static let clearance: CGFloat = 12
    /// Space kept between the fitted image and panels or the stage edge.
    public static let imageMargin: CGFloat = 16

    /// Default frames inside `stage`: Tools top-left, Colors bottom-left, History
    /// top-right, Layers bottom-right. When a side stack does not fit, the top
    /// panel (Tools or History) shortens down to its minimum and scrolls.
    public static func defaults(stage: CGRect, sizes: [PanelKind: CGSize] = [:]) -> [PanelKind: CGRect] {
        func size(_ k: PanelKind) -> CGSize { sizes[k] ?? k.defaultSize }
        var out: [PanelKind: CGRect] = [:]
        for (top, bottom, left) in [(PanelKind.tools, PanelKind.colors, true), (.history, .layers, false)] {
            let b = size(bottom)
            var t = size(top)
            let room = stage.height - 3 * clearance - b.height
            if t.height > room { t.height = max(top.minimumSize.height, room) }
            let tx = left ? stage.minX + clearance : stage.maxX - clearance - t.width
            let bx = left ? stage.minX + clearance : stage.maxX - clearance - b.width
            out[top] = CGRect(x: tx, y: stage.maxY - clearance - t.height, width: t.width, height: t.height)
            out[bottom] = CGRect(x: bx, y: stage.minY + clearance, width: b.width, height: b.height)
        }
        return out
    }

    /// The rectangle the document fits into: the stage less the space the visible
    /// panels take on its left and right, less 16 pt on every side.
    public static func clearRect(stage: CGRect, panels: [CGRect]) -> CGRect {
        var left: CGFloat = 0
        var right: CGFloat = 0
        for p in panels where p.intersects(stage) {
            if p.midX < stage.midX {
                left = max(left, p.maxX - stage.minX)
            } else {
                right = max(right, stage.maxX - p.minX)
            }
        }
        let width = max(stage.width - left - right - 2 * imageMargin, 1)
        let height = max(stage.height - 2 * imageMargin, 1)
        return CGRect(x: stage.minX + left + imageMargin, y: stage.minY + imageMargin, width: width, height: height)
    }

    /// True when no two frames touch or come closer than `gap`.
    public static func noOverlap(_ frames: [CGRect], gap: CGFloat = 0) -> Bool {
        for i in frames.indices {
            for j in frames.indices where j > i {
                if frames[i].insetBy(dx: -gap / 2, dy: -gap / 2).intersects(frames[j].insetBy(dx: -gap / 2, dy: -gap / 2)) {
                    return false
                }
            }
        }
        return true
    }

    /// Moves `frame` the shortest distance so it does not overlap `others` (kept
    /// `clearance` apart) and stays inside `bounds`. Returns it unchanged when it is
    /// already clear.
    public static func resolve(_ frame: CGRect, against others: [CGRect], in bounds: CGRect) -> CGRect {
        let gap = clearance
        func clear(_ r: CGRect) -> Bool {
            !others.contains { $0.insetBy(dx: -gap + 0.5, dy: -gap + 0.5).intersects(r) }
        }
        let start = clamp(frame, into: bounds)
        if clear(start) { return start }
        // Candidates: next to each other panel on all four sides, then the free
        // corners of the bounds.
        var candidates: [CGRect] = []
        for o in others {
            candidates.append(start.offsetBy(dx: 0, dy: o.minY - gap - start.maxY))
            candidates.append(start.offsetBy(dx: 0, dy: o.maxY + gap - start.minY))
            candidates.append(start.offsetBy(dx: o.minX - gap - start.maxX, dy: 0))
            candidates.append(start.offsetBy(dx: o.maxX + gap - start.minX, dy: 0))
        }
        for o in others {
            for dy in [o.minY - gap - start.maxY, o.maxY + gap - start.minY] {
                for dx in [o.minX - gap - start.maxX, o.maxX + gap - start.minX] {
                    candidates.append(start.offsetBy(dx: dx, dy: dy))
                }
            }
        }
        let fits = candidates.filter { bounds.contains($0) && clear($0) }
        func distance(_ r: CGRect) -> CGFloat { hypot(r.minX - start.minX, r.minY - start.minY) }
        if let best = fits.min(by: { distance($0) < distance($1) }) { return best }
        // Nothing fits: scan the bounds in 12 pt steps from the top-left.
        var y = bounds.maxY - start.height
        while y >= bounds.minY {
            var x = bounds.minX
            while x + start.width <= bounds.maxX {
                let r = CGRect(x: x, y: y, width: start.width, height: start.height)
                if clear(r) { return r }
                x += gap
            }
            y -= gap
        }
        return start
    }

    /// Keeps a frame inside `bounds`, shrinking it only when it is larger.
    public static func clamp(_ frame: CGRect, into bounds: CGRect) -> CGRect {
        var r = frame
        r.size.width = min(r.width, bounds.width)
        r.size.height = min(r.height, bounds.height)
        r.origin.x = min(max(r.minX, bounds.minX), bounds.maxX - r.width)
        r.origin.y = min(max(r.minY, bounds.minY), bounds.maxY - r.height)
        return r
    }

    /// True when the frame's title strip (its top 24 pt) is mostly on one screen.
    public static func isReachable(_ frame: CGRect, screens: [CGRect]) -> Bool {
        let title = CGRect(x: frame.minX, y: frame.maxY - 24, width: frame.width, height: 24)
        return screens.contains { s in
            let i = s.intersection(title)
            return !i.isNull && i.width >= min(60, title.width) && i.height >= 20
        }
    }

    /// Moves a frame that is not reachable onto the screen it overlaps most, or
    /// else the screen nearest to it, fully inside that screen's visible frame.
    public static func onScreen(_ frame: CGRect, screens: [CGRect]) -> CGRect {
        guard !screens.isEmpty, !isReachable(frame, screens: screens) else { return frame }
        func area(_ s: CGRect) -> CGFloat {
            let i = s.intersection(frame)
            return i.isNull ? 0 : i.width * i.height
        }
        func distance(_ s: CGRect) -> CGFloat { hypot(s.midX - frame.midX, s.midY - frame.midY) }
        let best = screens.max { a, b in
            area(a) != area(b) ? area(a) < area(b) : distance(a) > distance(b)
        }!
        return clamp(frame, into: best)
    }

    /// A key for the current display arrangement: the screen frames, sorted.
    public static func arrangementKey(_ screens: [CGRect]) -> String {
        screens.map { "\(Int($0.minX)),\(Int($0.minY)),\(Int($0.width)),\(Int($0.height))" }.sorted()
            .joined(separator: ";")
    }
}

/// Saved panel frames and visibility, one record per display arrangement.
public struct PanelStore {
    public let defaults: UserDefaults
    public let prefix: String

    public init(defaults: UserDefaults = .standard, prefix: String = "OPPanels") {
        self.defaults = defaults
        self.prefix = prefix
    }

    public struct Record: Equatable {
        public var frames: [PanelKind: CGRect]
        public var visible: Set<PanelKind>

        public init(frames: [PanelKind: CGRect], visible: Set<PanelKind>) {
            self.frames = frames
            self.visible = visible
        }
    }

    public func save(_ record: Record, arrangement: String) {
        var frames: [String: String] = [:]
        for (k, r) in record.frames { frames[k.rawValue] = NSStringFromRect(r) }
        let plist: [String: Any] = ["frames": frames, "visible": record.visible.map(\.rawValue).sorted()]
        defaults.set(plist, forKey: "\(prefix).\(arrangement)")
        defaults.set(arrangement, forKey: "\(prefix).last")
    }

    /// The record for this arrangement, or else the last one saved for any.
    public func load(arrangement: String) -> (record: Record, exact: Bool)? {
        if let r = record(forKey: "\(prefix).\(arrangement)") { return (r, true) }
        if let last = defaults.string(forKey: "\(prefix).last"), let r = record(forKey: "\(prefix).\(last)") {
            return (r, false)
        }
        return nil
    }

    public func clear() {
        for key in defaults.dictionaryRepresentation().keys where key.hasPrefix(prefix + ".") {
            defaults.removeObject(forKey: key)
        }
    }

    private func record(forKey key: String) -> Record? {
        guard let plist = defaults.dictionary(forKey: key), let raw = plist["frames"] as? [String: String] else {
            return nil
        }
        var frames: [PanelKind: CGRect] = [:]
        for (k, s) in raw {
            if let kind = PanelKind(rawValue: k) { frames[kind] = NSRectFromString(s) }
        }
        let visible = Set((plist["visible"] as? [String] ?? []).compactMap(PanelKind.init(rawValue:)))
        return Record(frames: frames, visible: visible)
    }
}
