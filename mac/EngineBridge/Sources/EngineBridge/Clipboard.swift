// The Edit menu's clipboard (M34): the `clipboard.copy` and
// `clipboard.profile_icc` queries for Copy, Copy Merged and Cut,
// `op_paste_begin` for Paste and Paste into New Layer (a move session: then
// moveUpdate, moveFinish and moveCancel), and `op_png_decode` to read a
// pasteboard PNG exactly (engine/op-ffi/src/clipboard.rs).
import COpenPainter
import Foundation

/// Pixels for the clipboard: straight RGBA8, row by row, top row first.
public struct ClipboardPixels: Equatable, Sendable {
    /// Where the pixels came from in the document (0, 0 for a decoded image).
    public var x: Int
    public var y: Int
    public var width: Int
    public var height: Int
    public var rgba: [UInt8]
    /// The ICC profile; nil or empty means sRGB.
    public var icc: Data?

    public init(x: Int = 0, y: Int = 0, width: Int, height: Int, rgba: [UInt8], icc: Data? = nil) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
        self.rgba = rgba
        self.icc = icc
    }
}

/// The answer to `pasteBegin`.
public struct PasteStart: Equatable, Sendable {
    public var session: UInt64
    /// The pasted profile's description when the pixels were converted to the
    /// document's profile; nil when the profiles match.
    public var convertedFrom: String?
}

extension Document {
    /// The active layer's pixels under the selection (the flattened image with
    /// `merged`), cropped to the selection, partial coverage as alpha, with the
    /// document's profile (sRGB bytes when it has none). Throws when the
    /// selection covers no pixel.
    public func copyPixels(merged: Bool) throws -> ClipboardPixels {
        let data = [UInt8](try queryData("clipboard.copy", ["merged": merged]))
        guard data.count >= 16 else { throw EngineError.badArgument }
        func u32(_ i: Int) -> Int {
            Int(UInt32(data[i]) | UInt32(data[i + 1]) << 8 | UInt32(data[i + 2]) << 16 | UInt32(data[i + 3]) << 24)
        }
        let (x, y, w, h) = (u32(0), u32(4), u32(8), u32(12))
        guard data.count == 16 + w * h * 4 else { throw EngineError.badArgument }
        let icc = try queryData("clipboard.profile_icc")
        return ClipboardPixels(x: x, y: y, width: w, height: h, rgba: Array(data[16...]), icc: icc)
    }

    /// Paste (or Paste into New Layer with `newLayer`): the pixels float at
    /// (`x`, `y`) in a move session, selected; pixels in another profile are
    /// converted to the document's. Throws `.busy` while another session runs.
    public func pasteBegin(_ pixels: ClipboardPixels, x: Int, y: Int, newLayer: Bool,
                           resampling: String = "bicubic") throws -> PasteStart {
        let params: [String: Any] = [
            "width": pixels.width, "height": pixels.height, "x": x, "y": y, "new_layer": newLayer,
            "resampling": resampling,
        ]
        let json = [UInt8](try JSONSerialization.data(withJSONObject: params))
        let icc = [UInt8](pixels.icc ?? Data())
        var out = OpBuffer(ptr: nil, len: 0)
        try engine.withHandle { h in
            try checkStatus(op_paste_begin(h, id, json, UInt(json.count), pixels.rgba, UInt(pixels.rgba.count),
                                           icc, UInt(icc.count), &out))
        }
        defer { op_buffer_free(&out) }
        guard let ptr = out.ptr,
              let answer = try JSONSerialization.jsonObject(with: Data(bytes: ptr, count: Int(out.len)))
                as? [String: Any]
        else { throw EngineError.badArgument }
        return PasteStart(session: (answer["session"] as? NSNumber)?.uint64Value ?? 0,
                          convertedFrom: answer["converted_from"] as? String)
    }
}

public enum PNGDecoder {
    /// Decodes a PNG exactly: straight RGBA8 (colors under zero alpha kept)
    /// and its ICC profile (nil when it has none). Safe on any thread.
    public static func decode(_ png: Data) throws -> ClipboardPixels {
        let bytes = [UInt8](png)
        var pixels = OpPixelBuffer(ptr: nil, len: 0, width: 0, height: 0)
        var icc = OpBuffer(ptr: nil, len: 0)
        let status = op_png_decode(bytes, UInt(bytes.count), &pixels, &icc)
        defer {
            op_pixels_free(&pixels)
            op_buffer_free(&icc)
        }
        try checkStatus(status)
        let rgba = pixels.ptr.map { [UInt8](UnsafeBufferPointer(start: $0, count: Int(pixels.len))) } ?? []
        let profile = icc.ptr.map { Data(bytes: $0, count: Int(icc.len)) }
        return ClipboardPixels(width: Int(pixels.width), height: Int(pixels.height), rgba: rgba,
                               icc: (profile?.isEmpty ?? true) ? nil : profile)
    }
}
