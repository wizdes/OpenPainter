// M50: encodes a flattened image for a save or for the save sheet's size
// preview, with the format's options, the metadata choice and the document
// resolution. PNG, JPEG and GIF go through the engine (op_encode_image); TIFF,
// HEIC, AVIF, WebP and BMP through ImageIO. SaveOptions.swift says why.
import Foundation

/// The flattened pixels of one document revision, ready to encode.
public struct SavePixels: Sendable {
    public var rgba: [UInt8]
    public var width: Int
    public var height: Int
    /// The document profile (empty: none).
    public var icc: Data
    /// Pixels per inch.
    public var dpi: Double

    public init(rgba: [UInt8], width: Int, height: Int, icc: Data, dpi: Double) {
        self.rgba = rgba
        self.width = width
        self.height = height
        self.icc = icc
        self.dpi = dpi
    }
}

public enum SaveEncoder {
    /// The file bytes for `type`. `metadata` is the imported metadata before
    /// the choice in `options` is applied.
    public static func encode(_ px: SavePixels, type: String, options raw: SaveOptions,
                              metadata: ImageMetadata?) throws -> Data {
        let options = raw.clamped(for: type)
        let kept = (metadata ?? ImageMetadata()).applying(options.metadata)
        let icc: Data? = px.icc.isEmpty ? nil : px.icc
        let dpi = px.dpi > 0 ? px.dpi : 96
        switch type {
        case "public.png":
            return try ImageEncoder.encode(
                params: ["format": "png", "png_level": options.pngLevel, "dpi": dpi], width: px.width,
                height: px.height, rgba: px.rgba, icc: icc, exif: kept.exifBlock(dpi: dpi), xmp: kept.xmp)
        case "public.jpeg":
            return try ImageEncoder.encode(
                params: ["format": "jpeg", "jpeg_quality": options.jpegQuality,
                         "jpeg_subsampling": options.jpegSubsampling.rawValue, "dpi": dpi],
                width: px.width, height: px.height, rgba: DocumentSaver.onWhite(px.rgba), icc: icc,
                exif: kept.exifBlock(dpi: dpi), xmp: kept.xmp)
        case "com.compuserve.gif":
            return try ImageEncoder.encode(
                params: ["format": "gif", "gif_dither": options.gifDither], width: px.width, height: px.height,
                rgba: px.rgba, icc: nil, exif: nil, xmp: nil)
        default:
            let kind = SaveOptionKind(type: type)
            let quality = kind.map { [.heic, .avif, .webp].contains($0) } == true
                ? Double(options.quality) / 100 : ImageIOExporter.defaultQuality
            return try ImageIOExporter.encode(
                rgba: px.rgba, width: px.width, height: px.height, icc: icc, type: type, quality: quality,
                metadata: kept.isEmpty ? nil : kept, dpi: dpi,
                tiffCompression: type == "public.tiff" ? options.tiffCompression.tag : nil)
        }
    }
}
