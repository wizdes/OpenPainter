// Builds the Remove Background test photos (M53) from sources.json.
//
//   swift tests/fixtures/remove-bg/make-fixtures.swift <cache-dir>
//
// For each fixture it downloads the cutouts (PNG with transparency, CC0 or
// public domain, from Wikimedia Commons) into <cache-dir>, checks their
// SHA-256, draws the generated background, puts each cutout on it and writes:
//   <name>.jpg       the composite photo (JPEG, quality 0.92)
//   <name>-mask.png  the true mask: the union of the cutouts' alpha, 8-bit gray
// The mask is exact for the composite: it is the alpha the cutouts were
// blended with. Backgrounds come from a seeded generator, so a rebuild gives
// the same bytes on the same macOS.
import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

struct Item: Decodable {
    let source: String  // key into "sources"
    let height: Double  // cutout height as a fraction of the canvas height
    let cx: Double      // cutout center, fractions of the canvas
    let cy: Double
}
struct Fixture: Decodable {
    let name: String
    let kind: String
    let width: Int
    let height: Int
    let background: String
    let seed: UInt64
    let items: [Item]
}
struct Source: Decodable {
    let title: String
    let page: String
    let url: String
    let sha256: String
    let license: String
    let author: String
}
struct Spec: Decodable {
    let sources: [String: Source]
    let fixtures: [Fixture]
}

struct Image {
    var w: Int, h: Int
    var px: [Double]  // straight RGBA, 0...1
}

struct Rng {
    var s: UInt64
    mutating func next() -> Double {
        s = s &* 6364136223846793005 &+ 1442695040888963407
        return Double((s >> 11) & ((1 << 53) - 1)) / Double(1 << 53)
    }
}

func download(_ url: String, to path: URL) throws {
    var req = URLRequest(url: URL(string: url)!)
    req.setValue("OpenPainterTestFixtures/1.0 (remove-bg fixtures)", forHTTPHeaderField: "User-Agent")
    let sem = DispatchSemaphore(value: 0)
    var result: Result<Data, Error> = .failure(NSError(domain: "download", code: 1))
    URLSession.shared.dataTask(with: req) { data, resp, err in
        if let data, (resp as? HTTPURLResponse)?.statusCode == 200 { result = .success(data) }
        else { result = .failure(err ?? NSError(domain: "download \(url)", code: (resp as? HTTPURLResponse)?.statusCode ?? 0)) }
        sem.signal()
    }.resume()
    sem.wait()
    try result.get().write(to: path)
}

func load(_ url: URL) -> Image {
    let src = CGImageSourceCreateWithURL(url as CFURL, nil)!
    let img = CGImageSourceCreateImageAtIndex(src, 0, nil)!
    return draw(img, img.width, img.height)
}

/// Draws `img` scaled into w x h (premultiplied, high quality) and returns straight RGBA.
func draw(_ img: CGImage, _ w: Int, _ h: Int) -> Image {
    var buf = [UInt8](repeating: 0, count: w * h * 4)
    buf.withUnsafeMutableBytes { p in
        let ctx = CGContext(data: p.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.interpolationQuality = .high
        ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
    }
    var out = Image(w: w, h: h, px: [Double](repeating: 0, count: w * h * 4))
    for i in 0..<(w * h) {
        let a = Double(buf[i * 4 + 3]) / 255
        for k in 0..<3 { out.px[i * 4 + k] = a > 0 ? min(1, Double(buf[i * 4 + k]) / 255 / a) : 0 }
        out.px[i * 4 + 3] = a
    }
    return out
}

func cgImage(_ im: Image) -> CGImage {
    let src = load8(im)
    return src.withUnsafeBytes { p in
        let provider = CGDataProvider(data: Data(p) as CFData)!
        return CGImage(width: im.w, height: im.h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: im.w * 4,
                       space: CGColorSpace(name: CGColorSpace.sRGB)!,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
    }
}
func load8(_ im: Image) -> [UInt8] { im.px.map { UInt8(max(0, min(255, ($0 * 255).rounded()))) } }

func write(_ img: CGImage, _ url: URL, jpeg: Double?) {
    let type = (jpeg == nil ? UTType.png : UTType.jpeg).identifier as CFString
    let d = CGImageDestinationCreateWithURL(url as CFURL, type, 1, nil)!
    var props: [CFString: Any] = [:]
    if let jpeg { props[kCGImageDestinationLossyCompressionQuality] = jpeg }
    CGImageDestinationAddImage(d, img, props as CFDictionary)
    precondition(CGImageDestinationFinalize(d), "could not write \(url.path)")
}

func mix(_ a: [Double], _ b: [Double], _ t: Double) -> [Double] { (0..<3).map { a[$0] * (1 - t) + b[$0] * t } }

/// The generated backgrounds. Each is a smooth base plus seeded noise.
func background(_ kind: String, _ w: Int, _ h: Int, _ seed: UInt64) -> Image {
    var rng = Rng(s: seed)
    var im = Image(w: w, h: h, px: [Double](repeating: 1, count: w * h * 4))
    // Bokeh circles are placed once, before the pixel loop.
    var blobs: [(x: Double, y: Double, r: Double, c: [Double])] = []
    if kind == "bokeh" {
        for _ in 0..<45 {
            blobs.append((rng.next() * Double(w), rng.next() * Double(h), 20 + rng.next() * 70,
                          [0.3 + rng.next() * 0.7, 0.2 + rng.next() * 0.6, 0.2 + rng.next() * 0.8]))
        }
    }
    for y in 0..<h {
        for x in 0..<w {
            let fx = Double(x) / Double(w), fy = Double(y) / Double(h)
            var c: [Double]
            switch kind {
            case "sky_field":
                c = fy < 0.62 ? mix([0.45, 0.66, 0.90], [0.80, 0.88, 0.95], fy / 0.62)
                    : mix([0.36, 0.55, 0.25], [0.24, 0.40, 0.16], (fy - 0.62) / 0.38)
            case "wood":
                let g = 0.5 + 0.5 * sin(fy * 90 + 6 * sin(fx * 7))
                c = mix([0.55, 0.36, 0.20], [0.70, 0.50, 0.30], g)
            case "slate":
                c = [0.18, 0.20, 0.23]
            case "wall":
                let line = (x % 96 < 2) || (y % 64 < 2)
                c = line ? [0.78, 0.74, 0.66] : mix([0.90, 0.85, 0.76], [0.84, 0.80, 0.72], fx)
            case "bokeh":
                c = mix([0.10, 0.12, 0.18], [0.20, 0.16, 0.12], fy)
                for b in blobs {
                    let d = hypot(Double(x) - b.x, Double(y) - b.y) / b.r
                    if d < 1.2 { c = mix(c, b.c, 0.45 * max(0, min(1, (1.2 - d) / 0.4))) }
                }
            case "studio":
                let d = hypot(fx - 0.5, fy - 0.45)
                c = mix([0.78, 0.82, 0.88], [0.45, 0.50, 0.58], min(1, d * 1.6))
            case "road":
                c = fy < 0.45 ? mix([0.55, 0.72, 0.92], [0.85, 0.90, 0.95], fy / 0.45)
                    : mix([0.42, 0.42, 0.44], [0.30, 0.30, 0.32], (fy - 0.45) / 0.55)
                if fy > 0.80 && fy < 0.82 && (x / 60) % 2 == 0 { c = [0.90, 0.80, 0.25] }
            case "brick":
                let row = y / 34, off = row % 2 == 0 ? 0 : 38
                let mortar = y % 34 < 4 || (x + off) % 76 < 4
                let tint = Double((row * 7 + (x + off) / 76 * 13) % 5) * 0.03
                c = mortar ? [0.72, 0.70, 0.66] : [0.58 + tint, 0.27 + tint / 2, 0.20]
            case "cloth":
                let weave = ((x / 3 + y / 3) % 2 == 0) ? 0.03 : -0.03
                c = [0.20 + weave, 0.45 + weave, 0.30 + weave]
            case "cream":
                c = mix([0.95, 0.93, 0.89], [0.92, 0.90, 0.86], fy)
            default:
                fatalError("unknown background \(kind)")
            }
            let n = (rng.next() - 0.5) * (kind == "cream" ? 0.03 : 0.07)
            let i = (y * w + x) * 4
            for k in 0..<3 { im.px[i + k] = max(0, min(1, c[k] + n)) }
            im.px[i + 3] = 1
        }
    }
    return im
}

let args = CommandLine.arguments
guard args.count == 2 else {
    print("usage: swift make-fixtures.swift <cache-dir>")
    exit(2)
}
let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let cache = URL(fileURLWithPath: args[1], isDirectory: true)
try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
let spec = try JSONDecoder().decode(Spec.self, from: Data(contentsOf: here.appendingPathComponent("sources.json")))

var cutouts: [String: CGImage] = [:]
for (key, s) in spec.sources {
    let file = cache.appendingPathComponent("\(key).png")
    if !FileManager.default.fileExists(atPath: file.path) { try download(s.url, to: file) }
    let digest = SHA256.hash(data: try Data(contentsOf: file)).map { String(format: "%02x", $0) }.joined()
    guard digest == s.sha256 else {
        print("SHA-256 mismatch for \(key): \(digest)")
        exit(1)
    }
    let src = CGImageSourceCreateWithURL(file as CFURL, nil)!
    cutouts[key] = CGImageSourceCreateImageAtIndex(src, 0, nil)!
}

for f in spec.fixtures {
    var out = background(f.background, f.width, f.height, f.seed)
    var mask = [Double](repeating: 0, count: f.width * f.height)
    for item in f.items {
        let cut = cutouts[item.source]!
        let ch = max(1, Int((item.height * Double(f.height)).rounded()))
        let cw = max(1, Int((Double(cut.width) * Double(ch) / Double(cut.height)).rounded()))
        let fg = draw(cut, cw, ch)
        let ox = Int((item.cx * Double(f.width)).rounded()) - cw / 2
        let oy = Int((item.cy * Double(f.height)).rounded()) - ch / 2
        for y in 0..<ch {
            // Bitmap context memory is top row first, like the output.
            let ty = oy + y
            guard ty >= 0, ty < f.height else { continue }
            for x in 0..<cw {
                let tx = ox + x
                guard tx >= 0, tx < f.width else { continue }
                let s = (y * cw + x) * 4, d = (ty * f.width + tx) * 4
                let a = fg.px[s + 3]
                for k in 0..<3 { out.px[d + k] = fg.px[s + k] * a + out.px[d + k] * (1 - a) }
                mask[ty * f.width + tx] = 1 - (1 - mask[ty * f.width + tx]) * (1 - a)
            }
        }
    }
    write(cgImage(out), here.appendingPathComponent("\(f.name).jpg"), jpeg: 0.92)
    var gray = mask.map { UInt8(max(0, min(255, ($0 * 255).rounded()))) }
    let maskImage = gray.withUnsafeMutableBytes { p in
        CGContext(data: p.baseAddress, width: f.width, height: f.height, bitsPerComponent: 8, bytesPerRow: f.width,
                  space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!.makeImage()!
    }
    write(maskImage, here.appendingPathComponent("\(f.name)-mask.png"), jpeg: nil)
    print("wrote \(f.name) (\(f.kind))")
}
