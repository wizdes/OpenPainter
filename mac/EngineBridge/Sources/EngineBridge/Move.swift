// Move Selected Pixels and Move Selection (M33): op_move_begin / update /
// finish / cancel. One session per tool use; only finish writes history.
import COpenPainter
import Foundation

/// What a move session moves.
public enum MoveKind: String, Sendable {
    /// The selected pixels (the whole layer with no selection) and the outline.
    case pixels
    /// Only the selection outline.
    case selection
}

/// One move transform: x' = a x + c y + tx, y' = b x + d y + ty, in document pixels.
public struct MoveTransform: Equatable, Sendable {
    public var a, b, c, d, tx, ty: Double

    public init(a: Double = 1, b: Double = 0, c: Double = 0, d: Double = 1, tx: Double = 0, ty: Double = 0) {
        self.a = a
        self.b = b
        self.c = c
        self.d = d
        self.tx = tx
        self.ty = ty
    }

    public static let identity = MoveTransform()

    public var array: [Double] { [a, b, c, d, tx, ty] }
}

extension Document {
    private func moveJSON(_ kind: MoveKind?, _ t: MoveTransform, _ resampling: String, _ copy: Bool) throws -> Data {
        var p: [String: Any] = ["transform": t.array, "resampling": resampling, "copy": copy]
        if let kind { p["kind"] = kind.rawValue }
        return try JSONSerialization.data(withJSONObject: p)
    }

    /// Starts a move on the active layer. `resampling`: nearest, bilinear,
    /// bicubic, super_sampling or best_quality. Throws `.busy` while a stroke,
    /// a preview or another move runs.
    public func moveBegin(_ kind: MoveKind, transform: MoveTransform = .identity, resampling: String = "bicubic",
                          copy: Bool = false) throws {
        let json = try moveJSON(kind, transform, resampling, copy)
        try engine.withHandle { h in
            try json.withUnsafeBytes { raw in
                try checkStatus(op_move_begin(h, id, raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count)))
            }
        }
    }

    /// Replaces the whole transform and publishes the preview. Returns the preview serial.
    @discardableResult
    public func moveUpdate(transform: MoveTransform, resampling: String = "bicubic", copy: Bool = false) throws
        -> UInt64
    {
        let json = try moveJSON(nil, transform, resampling, copy)
        var serial: UInt64 = 0
        try engine.withHandle { h in
            try json.withUnsafeBytes { raw in
                try checkStatus(
                    op_move_update(h, id, raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count), &serial))
            }
        }
        return serial
    }

    /// Commits the move as one history step.
    public func moveFinish() throws {
        try engine.withHandle { h in try checkStatus(op_move_finish(h, id)) }
    }

    /// Drops the move; pixels, selection and history stay as they were.
    public func moveCancel() throws {
        try engine.withHandle { h in try checkStatus(op_move_cancel(h, id)) }
    }
}
