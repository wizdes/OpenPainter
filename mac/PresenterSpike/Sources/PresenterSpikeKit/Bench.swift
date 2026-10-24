import AppKit
import Foundation

public struct BenchResult: CustomStringConvertible {
    public let path: String
    public let label: String
    public let frames: Int
    public let totalMs: [Double]
    public let renderMs: [Double]
    public let acquireMs: [Double]
    public let presentMs: [Double]
    public let blitMs: [Double]
    public let failures: Int

    static func med(_ v: [Double]) -> Double {
        guard !v.isEmpty else { return .nan }
        let s = v.sorted()
        return s.count % 2 == 1 ? s[s.count / 2] : (s[s.count / 2 - 1] + s[s.count / 2]) / 2
    }

    public var median: Double { BenchResult.med(totalMs) }
    public var worst: Double { totalMs.max() ?? .nan }

    public var description: String {
        String(format: "[%@] %@: total median %.2f ms worst %.2f ms | GPU render median %.2f worst %.2f | acquire median %.2f worst %.2f | present median %.2f worst %.2f | B blit GPU median %.2f worst %.2f | %d frames, %d failed",
               path, label, median, worst, BenchResult.med(renderMs), renderMs.max() ?? .nan,
               BenchResult.med(acquireMs), acquireMs.max() ?? .nan, BenchResult.med(presentMs), presentMs.max() ?? .nan,
               BenchResult.med(blitMs), blitMs.max() ?? .nan, frames, failures)
    }
}

public enum Bench {
    /// Frame times for a 6000 x 4000 document in a 1400 x 900 point view.
    /// "100%" is one document pixel per point (zoom = scale target pixels per document pixel).
    public static func run(_ h: Harness, doc: TestDocument, scale: Double, percent: Double,
                           frames: Int = 120, warmup: Int = 10) throws -> BenchResult {
        try h.setDocument(doc)
        var s = ViewState()
        s.widthPt = 1400
        s.heightPt = 900
        s.scale = scale
        s.zoom = scale * percent / 100
        let (pw, ph) = s.pixels
        s.originX = (Double(pw) - Double(doc.width) * s.zoom) / 2
        s.originY = (Double(ph) - Double(doc.height) * s.zoom) / 2
        try h.setView(s)
        var total: [Double] = [], render: [Double] = [], acquire: [Double] = [], present: [Double] = [], blit: [Double] = []
        var failed = 0
        for i in 0..<(frames + warmup) {
            let c = try h.renderAndCheck(checkColor: false)
            if !c.ok { failed += 1 }
            if i >= warmup {
                total.append(c.totalMs)
                render.append(c.renderMs)
                acquire.append(c.acquireMs)
                present.append(c.presentMs)
                blit.append(c.blitGpuMs)
            }
            Host.pump(0)
        }
        return BenchResult(path: h.pathName, label: "\(Int(percent))% at scale \(Int(scale)) (\(pw)x\(ph) px)",
                           frames: frames, totalMs: total, renderMs: render, acquireMs: acquire,
                           presentMs: present, blitMs: blit, failures: failed)
    }
}
