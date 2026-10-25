// M50 metadata choices: keep all, remove location (GPS) only, or remove all.
//
// Removing location takes the {GPS} dictionary out of the EXIF properties
// (so no GPS IFD, tag 34853, is written) and every XMP tag named GPS* (such as
// exif:GPSLatitude) out of the XMP packet, with CGImageMetadataRemoveTagWithPath
// (https://developer.apple.com/documentation/imageio/cgimagemetadataremovetagwithpath(_:_:_:)).
// No save writes an embedded thumbnail (kCGImageDestinationEmbedThumbnail is
// never set and the engine's encoders write none), so no thumbnail can carry
// GPS either. Removing all writes no EXIF, XMP, GPS or IPTC; the ICC profile
// and the resolution are not metadata and stay.
//
// The engine's PNG and JPEG encoders take EXIF as a raw TIFF-structure block.
// ImageIO builds it: a 1 × 1 PNG written with the kept dictionaries holds them
// as its eXIf chunk, whose bytes are that block.
import CoreGraphics
import Foundation
import ImageIO

extension ImageMetadata {
    /// The metadata a save keeps under `choice`.
    public func applying(_ choice: MetadataChoice) -> ImageMetadata {
        switch choice {
        case .keepAll: return self
        case .stripAll: return ImageMetadata()
        case .stripLocation:
            var props = properties
            props[kCGImagePropertyGPSDictionary as String] = nil
            return ImageMetadata(xmp: xmp.flatMap(Self.removingGPS), properties: props)
        }
    }

    /// The XMP packet without GPS* tags (nil when no tag is left).
    static func removingGPS(fromXMP data: Data) -> Data? {
        guard let md = CGImageMetadataCreateFromXMPData(data as CFData),
              let mutable = CGImageMetadataCreateMutableCopy(md) else { return nil }
        var paths: [String] = []
        var count = 0
        CGImageMetadataEnumerateTagsUsingBlock(mutable, nil, nil) { path, tag in
            count += 1
            let name = CGImageMetadataTagCopyName(tag) as String? ?? ""
            if name.hasPrefix("GPS") { paths.append(path as String) }
            return true
        }
        for p in paths { _ = CGImageMetadataRemoveTagWithPath(mutable, nil, p as CFString) }
        guard count > paths.count else { return nil }
        return CGImageMetadataCreateXMPData(mutable, nil) as Data?
    }

    /// The EXIF, TIFF, GPS and ExifAux dictionaries as a raw EXIF block (a
    /// TIFF header and IFDs), with the resolution set to `dpi` when given.
    /// Nil when there is nothing to write.
    public func exifBlock(dpi: Double?) -> Data? {
        let keys = [kCGImagePropertyExifDictionary, kCGImagePropertyExifAuxDictionary, kCGImagePropertyTIFFDictionary,
                    kCGImagePropertyGPSDictionary].map { $0 as String }
        var props: [String: Any] = [:]
        for k in keys { if let d = properties[k] { props[k] = d } }
        guard !props.isEmpty else { return nil }
        if let dpi { props = Self.withResolution(props, dpi: dpi) }
        let one: [UInt8] = [0, 0, 0, 255]
        guard let image = try? ImageIOExporter.cgImage(rgba: one, width: 1, height: 1, icc: nil) else { return nil }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out as CFMutableData, "public.png" as CFString, 1, nil)
        else { return nil }
        CGImageDestinationAddImage(dest, image, props as CFDictionary)
        guard CGImageDestinationFinalize(dest) else { return nil }
        return PNGChunks.first("eXIf", in: out as Data)
    }

    /// `props` with the TIFF dictionary's XResolution, YResolution and
    /// ResolutionUnit (2, inches) set to `dpi`, when there is a TIFF dictionary.
    static func withResolution(_ props: [String: Any], dpi: Double) -> [String: Any] {
        var p = props
        let key = kCGImagePropertyTIFFDictionary as String
        if var tiff = p[key] as? [String: Any] {
            tiff[kCGImagePropertyTIFFXResolution as String] = dpi
            tiff[kCGImagePropertyTIFFYResolution as String] = dpi
            tiff[kCGImagePropertyTIFFResolutionUnit as String] = 2
            p[key] = tiff
        }
        return p
    }
}

/// Reads PNG chunks (length, type, data, CRC).
public enum PNGChunks {
    /// Every chunk type and its data, in file order.
    public static func all(_ data: Data) -> [(type: String, data: Data)] {
        let b = [UInt8](data)
        guard b.count >= 8, b.prefix(8) == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] else { return [] }
        var out: [(String, Data)] = []
        var p = 8
        while p + 12 <= b.count {
            let n = Int(b[p]) << 24 | Int(b[p + 1]) << 16 | Int(b[p + 2]) << 8 | Int(b[p + 3])
            guard n >= 0, p + 12 + n <= b.count else { break }
            let type = String(bytes: b[p + 4..<p + 8], encoding: .ascii) ?? "????"
            out.append((type, Data(b[p + 8..<p + 8 + n])))
            p += 12 + n
        }
        return out
    }

    public static func first(_ type: String, in data: Data) -> Data? {
        all(data).first { $0.type == type }?.data
    }
}
