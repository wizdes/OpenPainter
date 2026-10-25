// Pure geometry for the Move tools (M33): what a press hits (a nub, the
// pivot, the box, the rotation corridor or nothing), and how a drag turns the
// session's transform. No AppKit state, so tests can check every rule.
//
// Rules follow paint.net's Move Tools page
// (https://www.getpaint.net/doc/latest/MoveTools.html, served from
// paint.net/doc/latest/MoveTools.html), with Ctrl as Command and Alt as Option:
//   drag inside the box          move (whole document pixels)
//   drag a Control Nub           scale; past the opposite nub flips
//     Shift                      keep the original aspect ratio
//     Option (paint.net Alt)     scale about the center
//   drag in the corridor just outside the box, or with the right button
//   anywhere                     rotate about the pivot
//     Shift                      the angle snaps to multiples of 15 degrees
//   drag the pivot (circle with a cross)   move the center of rotation
//   Command (paint.net Ctrl)     work on a copy, the original pixels stay
//   arrow keys                   nudge 1 px; Command (paint.net Ctrl) 10 px
//
// The session keeps the original box (`base`, document pixels) and one
// cumulative transform `m` (x' = a x + c y + tx, y' = b x + d y + ty, the
// engine's convention). The box on screen is `m` applied to `base`. A drag
// starts from the transform at the press, so releasing and dragging again
// continues where the last drag ended.
import AppKit

/// What a press on the canvas hits with a Move tool.
public enum MoveHit: Equatable, Sendable {
    case nub(RectHandle)
    case pivot
    case inside
    /// The rotation corridor just outside the box.
    case rotate
    case outside
}

/// One Move tools session: the original box, the transform so far, the pivot
/// and the rotation angle.
public struct MoveState: Equatable, Sendable {
    /// The box at the start of the session (document pixels).
    public var base: CGRect
    /// The cumulative transform.
    public var m: CGAffineTransform = .identity
    /// The pivot in the base box's frame (it follows the box).
    public var pivotLocal: CGPoint
    /// The rotation so far, in degrees (Shift snaps this total).
    public var angle: Double = 0

    public init(base: CGRect) {
        self.base = base
        pivotLocal = CGPoint(x: base.midX, y: base.midY)
    }

    /// The pivot in document pixels.
    public var pivot: CGPoint { pivotLocal.applying(m) }

    /// The box corners in document pixels: top left, top right, bottom right, bottom left.
    public var corners: [CGPoint] {
        [CGPoint(x: base.minX, y: base.minY), CGPoint(x: base.maxX, y: base.minY),
         CGPoint(x: base.maxX, y: base.maxY), CGPoint(x: base.minX, y: base.maxY)].map { $0.applying(m) }
    }

    /// Where each nub sits in document pixels.
    public func nub(_ h: RectHandle) -> CGPoint { h.point(in: base).applying(m) }

    /// The transform as the engine takes it.
    public var engine: MoveTransform {
        MoveTransform(a: m.a, b: m.b, c: m.c, d: m.d, tx: m.tx, ty: m.ty)
    }
}

/// A drag in progress: what it grabbed and the state at the press.
public struct MoveDrag: Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        case move, rotate, pivot
        case scale(RectHandle)
    }
    public var kind: Kind
    /// The press in document pixels.
    public var start: CGPoint
    /// The session state at the press.
    public var from: MoveState

    public init(kind: Kind, start: CGPoint, from: MoveState) {
        self.kind = kind
        self.start = start
        self.from = from
    }
}

public enum MoveGeometry {
    /// Width of the rotation corridor outside the box, in view points.
    public static let corridor: CGFloat = 24
    /// The pivot's hit radius in view points.
    public static let pivotRadius: CGFloat = 10
    /// Shift snaps the rotation to this many degrees.
    public static let snapDegrees: Double = 15

    /// What a press at `v` (view points) hits. `toView` maps document pixels
    /// to view points.
    public static func hit(_ v: CGPoint, state: MoveState, toView: (CGPoint) -> CGPoint) -> MoveHit {
        let quad = state.corners.map(toView)
        let side = min(hypot(quad[1].x - quad[0].x, quad[1].y - quad[0].y),
                       hypot(quad[3].x - quad[0].x, quad[3].y - quad[0].y))
        // 24 pt targets, smaller on a small box so the inside stays reachable.
        let half = min(12, max(4, side / 4))
        for h in RectHandle.allCases {
            let c = toView(state.nub(h))
            if abs(c.x - v.x) <= half, abs(c.y - v.y) <= half { return .nub(h) }
        }
        let p = toView(state.pivot)
        if hypot(p.x - v.x, p.y - v.y) <= pivotRadius { return .pivot }
        if contains(quad, v) { return .inside }
        if distance(v, toQuad: quad) <= corridor { return .rotate }
        return .outside
    }

    /// True when `p` is inside the convex quad (either winding).
    static func contains(_ q: [CGPoint], _ p: CGPoint) -> Bool {
        var sign: CGFloat = 0
        for i in 0..<q.count {
            let a = q[i], b = q[(i + 1) % q.count]
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
            if cross == 0 { continue }
            if sign == 0 { sign = cross } else if (cross > 0) != (sign > 0) { return false }
        }
        return true
    }

    static func distance(_ p: CGPoint, toQuad q: [CGPoint]) -> CGFloat {
        var best = CGFloat.infinity
        for i in 0..<q.count {
            let a = q[i], b = q[(i + 1) % q.count]
            let ab = CGPoint(x: b.x - a.x, y: b.y - a.y)
            let len = ab.x * ab.x + ab.y * ab.y
            let t = len > 0 ? max(0, min(1, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len)) : 0
            best = min(best, hypot(a.x + t * ab.x - p.x, a.y + t * ab.y - p.y))
        }
        return best
    }

    /// The state after dragging to `p` (document pixels).
    public static func drag(_ d: MoveDrag, to p: CGPoint, shift: Bool, option: Bool) -> MoveState {
        var s = d.from
        switch d.kind {
        case .move:
            let dx = (p.x - d.start.x).rounded(), dy = (p.y - d.start.y).rounded()
            s.m = s.m.concatenating(CGAffineTransform(translationX: dx, y: dy))
        case .rotate:
            let c = d.from.pivot
            let a0 = atan2(d.start.y - c.y, d.start.x - c.x)
            let a1 = atan2(p.y - c.y, p.x - c.x)
            var delta = (a1 - a0) * 180 / .pi
            if delta > 180 { delta -= 360 }
            if delta < -180 { delta += 360 }
            var total = d.from.angle + delta
            if shift { total = (total / snapDegrees).rounded() * snapDegrees }
            s = rotated(d.from, by: total - d.from.angle)
        case .pivot:
            if let inv = invertible(d.from.m) { s.pivotLocal = p.applying(inv) }
        case let .scale(h):
            guard let inv = invertible(d.from.m) else { return s }
            let q = p.applying(inv), q0 = d.start.applying(inv)
            let r = d.from.base
            // The fixed point: the opposite edge, or the center with Option.
            let ax = option ? r.midX : (h.unit.x == 0 ? r.maxX : r.minX)
            let ay = option ? r.midY : (h.unit.y == 0 ? r.maxY : r.minY)
            var sx: CGFloat = 1, sy: CGFloat = 1
            if h.movesX, q0.x != ax { sx = (q.x - ax) / (q0.x - ax) }
            if h.movesY, q0.y != ay { sy = (q.y - ay) / (q0.y - ay) }
            if shift {
                let k = h.movesX && h.movesY ? max(abs(sx), abs(sy)) : (h.movesX ? abs(sx) : abs(sy))
                sx = k * (sx < 0 ? -1 : 1)
                sy = k * (sy < 0 ? -1 : 1)
            }
            // Keep at least one pixel so the transform stays invertible.
            sx = keepPixel(sx, r.width)
            sy = keepPixel(sy, r.height)
            // An edge nub with Shift scales the other axis about the box's middle.
            let fx = h.movesX ? ax : r.midX, fy = h.movesY ? ay : r.midY
            let local = CGAffineTransform(translationX: -fx, y: -fy)
                .concatenating(CGAffineTransform(scaleX: sx, y: sy))
                .concatenating(CGAffineTransform(translationX: fx, y: fy))
            s.m = local.concatenating(d.from.m)
        }
        return s
    }

    /// `s` turned by `degrees` about its pivot.
    public static func rotated(_ s: MoveState, by degrees: Double) -> MoveState {
        var out = s
        let c = s.pivot
        let t = CGAffineTransform(translationX: -c.x, y: -c.y)
            .concatenating(CGAffineTransform(rotationAngle: degrees * .pi / 180))
            .concatenating(CGAffineTransform(translationX: c.x, y: c.y))
        out.m = s.m.concatenating(t)
        out.angle = s.angle + degrees
        return out
    }

    /// `s` moved by whole pixels (arrow-key nudges).
    public static func nudged(_ s: MoveState, dx: CGFloat, dy: CGFloat) -> MoveState {
        var out = s
        out.m = s.m.concatenating(CGAffineTransform(translationX: dx, y: dy))
        return out
    }

    /// The nudge for an arrow key: 1 px, or 10 px with Command (paint.net's Ctrl).
    public static func nudge(keyCode: UInt16, modifiers: NSEvent.ModifierFlags) -> CGVector? {
        let step: CGFloat = modifiers.contains(.command) ? 10 : 1
        switch keyCode {
        case 123: return CGVector(dx: -step, dy: 0)
        case 124: return CGVector(dx: step, dy: 0)
        case 125: return CGVector(dx: 0, dy: step)
        case 126: return CGVector(dx: 0, dy: -step)
        default: return nil
        }
    }

    static func keepPixel(_ s: CGFloat, _ size: CGFloat) -> CGFloat {
        let minimum = 1 / max(size, 1)
        if abs(s) >= minimum { return s }
        return s < 0 ? -minimum : minimum
    }

    static func invertible(_ m: CGAffineTransform) -> CGAffineTransform? {
        let det = m.a * m.d - m.b * m.c
        guard det.isFinite, abs(det) > 1e-12 else { return nil }
        return m.inverted()
    }
}
