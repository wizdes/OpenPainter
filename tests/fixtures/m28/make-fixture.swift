// Writes the M28 Phase 2 end-to-end fixture: a small JPEG with EXIF
// orientation 6 and a Display P3 profile, and next to it the pixels ImageIO
// decodes from it (the app's JPEG decoder) as a PNG.
//
//   swift tests/fixtures/m28/make-fixture.swift tests/fixtures/m28/phase2-exif6-p3.jpg
//
// writes phase2-exif6-p3.jpg and phase2-exif6-p3.imageio.png.
//
// The stored image is 192 x 256 (portrait). Orientation 6 means "turn 90
// degrees clockwise to show", so the image opens as 256 x 192 (landscape).
// The stored top-left 32 x 32 corner is a pure green block; after the turn it
// sits at the shown top-right, which the tests use to check the rotation.
//
// Pixel values are Display P3 numbers (no color conversion happens on write:
// the CGImage is made in Display P3). ImageIO writes the JPEG at quality 1.0,
// which macOS encodes without chroma subsampling (4:4:4).
//
// Why the PNG: the app decodes JPEG with ImageIO (M17), opcli with the
// engine's decoder (zune-jpeg). They differ by up to 3 levels in about 13% of
// the channel samples on this image (measured on macOS 27), so the engine
// scenario starts from ImageIO's pixels: the decode below is the app's
// (CGImageSourceCreateImageAtIndex, drawn as RGBA8 in the image's own color
// space, turned by the orientation). Phase2EndToEndTests checks that the
// app's open gives exactly these bytes, and opcli checks its own decode stays
// within the scenario's `decoder_max_diff`.
import CoreGraphics
import Foundation
import ImageIO

let args = CommandLine.arguments
guard args.count == 2 else {
    FileHandle.standardError.write(Data("usage: make-fixture.swift <out.jpg>\n".utf8))
    exit(2)
}
let width = 192, height = 256
var px = [UInt8](repeating: 255, count: width * height * 4)
for y in 0..<height {
    for x in 0..<width {
        let i = (y * width + x) * 4
        let fx = Double(x) / Double(width), fy = Double(y) / Double(height)
        var r = 127.5 + 110 * sin(fx * 6.0 + fy * 2.0)
        var g = 127.5 + 110 * cos(fy * 5.0 - fx * 1.5)
        var b = 127.5 + 110 * sin((fx + fy) * 4.0 + 1.0)
        // A soft disc and a hard-edged bar give the effects edges to work on.
        let d = hypot(Double(x) - 120, Double(y) - 150)
        if d < 40 { let t = 1 - d / 40; r = r * (1 - t) + 250 * t; g *= 1 - 0.6 * t; b *= 1 - 0.6 * t }
        if y >= 200, y < 216, x >= 40 { r = 30; g = 40; b = 200 }
        if x < 32, y < 32 { r = 0; g = 255; b = 0 }  // the orientation marker
        px[i] = UInt8(max(0, min(255, r.rounded())))
        px[i + 1] = UInt8(max(0, min(255, g.rounded())))
        px[i + 2] = UInt8(max(0, min(255, b.rounded())))
    }
}
let space = CGColorSpace(name: CGColorSpace.displayP3)!
let provider = CGDataProvider(data: Data(px) as CFData)!
let image = CGImage(
    width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider, decode: nil,
    shouldInterpolate: false, intent: .defaultIntent)!
let url = URL(fileURLWithPath: args[1])
guard let dest = CGImageDestinationCreateWithURL(url as CFURL, "public.jpeg" as CFString, 1, nil) else {
    FileHandle.standardError.write(Data("cannot create \(url.path)\n".utf8))
    exit(1)
}
let props: [CFString: Any] = [
    kCGImageDestinationLossyCompressionQuality: 1.0,
    kCGImagePropertyOrientation: 6,
    kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFOrientation: 6],
    kCGImagePropertyExifDictionary: [kCGImagePropertyExifUserComment: "OpenPainter M28 fixture"],
]
CGImageDestinationAddImage(dest, image, props as CFDictionary)
guard CGImageDestinationFinalize(dest) else {
    FileHandle.standardError.write(Data("cannot write \(url.path)\n".utf8))
    exit(1)
}
print("wrote \(url.path)")

// The ImageIO decode, turned by orientation 6 (90 degrees clockwise).
let source = CGImageSourceCreateWithURL(url as CFURL, nil)!
let props2 = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as! [CFString: Any]
precondition((props2[kCGImagePropertyOrientation] as? NSNumber)?.intValue == 6, "orientation 6 was written")
let decoded = CGImageSourceCreateImageAtIndex(source, 0, nil)!
let dspace = decoded.colorSpace!
var stored = [UInt8](repeating: 0, count: width * height * 4)
let ctx = CGContext(data: &stored, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                    space: dspace, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.draw(decoded, in: CGRect(x: 0, y: 0, width: width, height: height))
let (tw, th) = (height, width)
var turned = [UInt8](repeating: 0, count: width * height * 4)
for y in 0..<height {
    for x in 0..<width {
        let (nx, ny) = (height - 1 - y, x)
        for c in 0..<4 { turned[(ny * tw + nx) * 4 + c] = stored[(y * width + x) * 4 + c] }
    }
}
let turnedImage = CGImage(
    width: tw, height: th, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: tw * 4, space: dspace,
    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
    provider: CGDataProvider(data: Data(turned) as CFData)!, decode: nil, shouldInterpolate: false,
    intent: .defaultIntent)!
let pngURL = url.deletingPathExtension().appendingPathExtension("imageio.png")
let png = CGImageDestinationCreateWithURL(pngURL as CFURL, "public.png" as CFString, 1, nil)!
CGImageDestinationAddImage(png, turnedImage, nil)
guard CGImageDestinationFinalize(png) else {
    FileHandle.standardError.write(Data("cannot write \(pngURL.path)\n".utf8))
    exit(1)
}
print("wrote \(pngURL.path)")
