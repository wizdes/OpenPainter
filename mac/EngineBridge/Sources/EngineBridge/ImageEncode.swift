// M50 save options: the engine's PNG, JPEG and GIF encoders with the save
// sheet's options, the resolution and the kept metadata (op_encode_image), and
// pixel import at the file's resolution (op_import_pixels_at).
import COpenPainter
import Foundation

public enum ImageEncoder {
    /// Encodes straight RGBA8 pixels. `params` holds the options, such as
    /// `["format": "jpeg", "jpeg_quality": 95, "jpeg_subsampling": "422", "dpi": 300.0]`
    /// (openpainter.h, op_encode_image). `exif` is a raw TIFF-structure EXIF
    /// block; `xmp` an XMP packet. Safe to call from any thread.
    public static func encode(params: [String: any Sendable], width: Int, height: Int, rgba: [UInt8],
                              icc: Data?, exif: Data?, xmp: Data?) throws -> Data {
        guard width > 0, height > 0, rgba.count == width * height * 4 else { throw EngineError.badArgument }
        let json = [UInt8](try JSONSerialization.data(withJSONObject: params))
        let iccBytes = [UInt8](icc ?? Data())
        let exifBytes = [UInt8](exif ?? Data())
        let xmpBytes = [UInt8](xmp ?? Data())
        var out = OpBuffer(ptr: nil, len: 0)
        let status = op_encode_image(json, UInt(json.count), UInt32(width), UInt32(height), rgba, UInt(rgba.count),
                                     iccBytes, UInt(iccBytes.count), exifBytes, UInt(exifBytes.count),
                                     xmpBytes, UInt(xmpBytes.count), &out)
        defer { op_buffer_free(&out) }
        try checkStatus(status)
        guard let ptr = out.ptr else { return Data() }
        return Data(bytes: ptr, count: Int(out.len))
    }
}

extension Engine {
    /// `importPixels` at `dpi` pixels per inch (the file's resolution).
    public func importPixels(width: UInt32, height: UInt32, rgba: [UInt8], icc: [UInt8]?, dpi: Double) throws -> Document {
        var id: UInt64 = 0
        let profile = icc ?? []
        try withHandle { h in
            try checkStatus(op_import_pixels_at(h, width, height, rgba, UInt(rgba.count), profile,
                                                UInt(profile.count), dpi, &id))
        }
        return Document(engine: self, id: id)
    }
}
