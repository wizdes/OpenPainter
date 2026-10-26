// M19 formats: the native .opaint file, the flat formats ImageIO opens (JPEG,
// TIFF, GIF, WebP, BMP), and what Save, Save As and Save a Copy write.
//
// The engine writes .opaint and PNG itself (op_doc_save). ImageIO writes JPEG,
// BMP, TIFF, GIF, HEIC, and WebP and AVIF only where
// CGImageDestinationCopyTypeIdentifiers() lists them, decided at run time as
// M17 does for AVIF
// (https://developer.apple.com/documentation/imageio/cgimagedestinationcopytypeidentifiers()).
// Every format but .opaint holds one layer, so a layered image shows the
// flatten prompt first; the open document keeps its layers, as paint.net does
// (https://www.getpaint.net/doc/latest/FileMenu.html, Save and Save All).
import Foundation
import ImageIO
import UniformTypeIdentifiers

extension ImageFormats {
    /// The native format's type identifier (UTExportedTypeDeclarations in the app's Info.plist).
    public static let opaintIdentifier = "dev.yili.openpainter.opaint"
    public static let opaintExtension = "opaint"

    public static var opaintType: UTType {
        UTType(opaintIdentifier) ?? UTType(exportedAs: opaintIdentifier, conformingTo: .data)
    }

    /// The flat formats ImageIO opens for M19, in paint.net's list order.
    public static let flatImportIdentifiers = [
        "public.jpeg", "com.microsoft.bmp", "com.compuserve.gif", "public.tiff", "org.webmproject.webp",
    ]

    /// JPEG, BMP, GIF, TIFF and WebP, limited to what this macOS can decode.
    public static func flatImportTypes(sourceTypes: [String] = systemSourceTypes) -> [UTType] {
        flatImportIdentifiers.filter(sourceTypes.contains).compactMap(UTType.init)
    }

    /// The ImageIO formats Save As offers, in order, limited to what this Mac can encode.
    public static func imageIOSaveTypes(destinationTypes: [String] = systemDestinationTypes) -> [String] {
        ["public.jpeg", "com.microsoft.bmp", "public.tiff", "com.compuserve.gif", "org.webmproject.webp",
         "public.heic", "public.avif"].filter(destinationTypes.contains)
    }

    /// Every type Save, Save As and Save a Copy write: .opaint and PNG through
    /// the engine, then the ImageIO formats.
    public static func saveTypes(destinationTypes: [String] = systemDestinationTypes) -> [String] {
        [opaintIdentifier, "public.png"] + imageIOSaveTypes(destinationTypes: destinationTypes)
    }

    /// True for formats that hold one layer (all but .opaint).
    public static func isFlat(_ type: String) -> Bool { type != opaintIdentifier }

    /// A short name for messages, such as "JPEG" or "OPAINT".
    public static func shortName(_ type: String) -> String {
        if type == opaintIdentifier { return "OpenPainter" }
        if type == "public.jpeg" { return "JPEG" }
        if type == "public.tiff" { return "TIFF" }
        return UTType(type)?.preferredFilenameExtension?.uppercased() ?? type
    }

    /// The file name extension for a save type.
    public static func fileExtension(_ type: String) -> String? {
        type == opaintIdentifier ? opaintExtension : UTType(type)?.preferredFilenameExtension
    }

    /// Fix round 3, item 5: why plain Save must not write `url` back, as
    /// status-bar notes (empty when it may): content whose type differs
    /// from the one its name implies (a PNG named .jpg would become a
    /// JPEG), more than one frame or page (an animated GIF, a multi-page
    /// TIFF: Save writes one), or a color model OpenPainter converts to RGB
    /// (CMYK, Lab). Open, Revert and Recover use it. It reads the file's
    /// header only.
    public static func plainSaveReasons(_ url: URL, contentType: String) -> [String] {
        var reasons: [String] = []
        let ext = url.pathExtension
        if let named = saveType(forExtension: ext), named != contentType {
            reasons.append("This file is a \(shortName(contentType)) image named .\(ext); "
                + "Save asks where to save so it is not rewritten as \(shortName(named)).")
        }
        guard contentType != opaintIdentifier,
              let source = CGImageSourceCreateWithURL(url as CFURL, nil)
        else { return reasons }
        // Fix round 4, item 1: only formats that hold animation or pages
        // count. A camera JPEG with an MPF preview or a depth image may
        // report more than one image, and plain Save keeps working on it.
        let frames = framedTypes.contains(contentType) ? CGImageSourceGetCount(source) : 1
        if frames > 1 {
            reasons.append("This file holds \(frames) frames or pages; OpenPainter opened the first. "
                + "Save asks where to save so the others are kept.")
        }
        let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        if let model = props?[kCGImagePropertyColorModel as String] as? String,
           model != (kCGImagePropertyColorModelRGB as String), model != (kCGImagePropertyColorModelGray as String) {
            reasons.append("This file is \(model); OpenPainter converted it to RGB. "
                + "Save asks where to save so the \(model) original is kept.")
        }
        return reasons
    }

    /// Fix round 4, item 1: the types whose extra images are frames or
    /// pages that plain Save would drop (GIF, TIFF, APNG, WebP).
    static let framedTypes: Set<String> = [
        "com.compuserve.gif", "public.tiff", "public.png", "org.webmproject.webp",
    ]

    /// The type a URL saves as, from its extension (used when a type name is a
    /// dynamic or unknown identifier).
    public static func saveType(forExtension ext: String) -> String? {
        let e = ext.lowercased()
        if e == opaintExtension { return opaintIdentifier }
        guard let t = UTType(filenameExtension: e) else { return nil }
        return saveTypes().first { UTType($0).map { t.conforms(to: $0) } ?? false }
    }

    /// A ZIP whose first entry is a stored "mimetype" holding image/openraster
    /// (the OpenRaster rule the .opaint writer follows).
    static func isOpaint(_ head: Data) -> Bool {
        let b = [UInt8](head)
        guard b.count >= 38, b[0] == 0x50, b[1] == 0x4B, b[2] == 0x03, b[3] == 0x04 else { return false }
        let nameLength = Int(b[26]) | Int(b[27]) << 8
        let extraLength = Int(b[28]) | Int(b[29]) << 8
        guard nameLength == 8, String(bytes: b[30..<38], encoding: .ascii) == "mimetype" else { return false }
        let start = 38 + extraLength
        let mime = Array("image/openraster".utf8)
        guard b.count >= start + mime.count else { return false }
        return Array(b[start..<start + mime.count]) == mime
    }
}
