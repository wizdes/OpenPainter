import COpPresentSpike
import CoreGraphics
import Foundation

public enum ColorTolerance {
    /// Path A pass bound: CIEDE2000 against the ColorSync reference, per patch.
    /// 1.0 is about one just-noticeable difference.
    public static let deltaE00 = 1.0
}

/// Where path A's color error comes from, in 1/255 levels (worst over patches).
public struct ColorSplit: CustomStringConvertible {
    /// lcms2 float transform vs ColorSync (two color engines disagreeing).
    public var lcmsVsColorSync = 0.0
    /// The 33^3 table (trilinear, CPU) vs the lcms2 float transform.
    public var tableVsLcms = 0.0
    /// The GPU drawable byte vs the CPU table value (shader and 8-bit rounding).
    public var gpuVsTable = 0.0

    mutating func merge(_ o: ColorSplit) {
        lcmsVsColorSync = max(lcmsVsColorSync, o.lcmsVsColorSync)
        tableVsLcms = max(tableVsLcms, o.tableVsLcms)
        gpuVsTable = max(gpuVsTable, o.gpuVsTable)
    }

    public var description: String {
        String(format: "split{lcms2-vs-ColorSync %.2f, table-vs-lcms2 %.2f, GPU-vs-table %.2f}",
               lcmsVsColorSync, tableVsLcms, gpuVsTable)
    }

    struct Analysis {
        let exact: [[Float]]
        let table: [[Float]]
        let summary: ColorSplit
    }

    private static var cache: [String: Analysis] = [:]

    static func analyse(_ doc: TestDocument, display: CGColorSpace) throws -> Analysis {
        let displayICC = display.copyICCData()! as Data
        let key = "\(doc.name)|\(displayICC.hashValue)"
        if let hit = cache[key] { return hit }
        let input: [Float] = doc.patches.flatMap { [Float($0.rgb.0) / 255, Float($0.rgb.1) / 255, Float($0.rgb.2) / 255] }
        var exact = [Float](repeating: 0, count: input.count)
        var table = [Float](repeating: 0, count: input.count)
        _ = try doc.icc.withUnsafeBytes { d in
            try displayICC.withUnsafeBytes { s in
                try check(op_ps_debug_convert(d.bindMemory(to: UInt8.self).baseAddress, d.count,
                                              s.bindMemory(to: UInt8.self).baseAddress, s.count,
                                              input, doc.patches.count, &exact, &table), "debug_convert")
            }
        }
        let reference = Reference.expected(doc, display: display)
        var sum = ColorSplit()
        var e: [[Float]] = [], t: [[Float]] = []
        for i in 0..<doc.patches.count {
            let ex = Array(exact[i * 3..<i * 3 + 3]), tb = Array(table[i * 3..<i * 3 + 3])
            e.append(ex)
            t.append(tb)
            for k in 0..<3 {
                sum.lcmsVsColorSync = max(sum.lcmsVsColorSync, Double(abs(ex[k] - reference[i][k])) * 255)
                sum.tableVsLcms = max(sum.tableVsLcms, Double(abs(tb[k] - ex[k])) * 255)
            }
        }
        let a = Analysis(exact: e, table: t, summary: sum)
        cache[key] = a
        return a
    }
}

/// CIEDE2000 between two colors given in the same RGB space; Lab from ColorSync.
public enum DeltaE {
    static let lab = CGColorSpace(name: CGColorSpace.genericLab)!
    private static var cache: [String: [Double]] = [:]

    static func toLab(_ space: CGColorSpace, _ rgb: [Double]) -> [Double] {
        let key = "\(space.name as String? ?? "icc\((space.copyICCData()! as Data).hashValue)")|\(rgb)"
        if let hit = cache[key] { return hit }
        let color = CGColor(colorSpace: space, components: rgb.map { CGFloat($0) } + [1])!
        let converted = color.converted(to: lab, intent: .relativeColorimetric, options: nil)!
        let out = converted.components!.prefix(3).map(Double.init)
        cache[key] = out
        return out
    }

    public static func de00(_ space: CGColorSpace, _ a: [Double], _ b: [Double]) -> Double {
        de00(toLab(space, a), toLab(space, b))
    }

    /// Sharma, Wu and Dalal (2005) formulation.
    public static func de00(_ x: [Double], _ y: [Double]) -> Double {
        let (l1, a1, b1) = (x[0], x[1], x[2]), (l2, a2, b2) = (y[0], y[1], y[2])
        let c1 = hypot(a1, b1), c2 = hypot(a2, b2), cbar = (c1 + c2) / 2
        let g = 0.5 * (1 - sqrt(pow(cbar, 7) / (pow(cbar, 7) + pow(25, 7))))
        let a1p = (1 + g) * a1, a2p = (1 + g) * a2
        let c1p = hypot(a1p, b1), c2p = hypot(a2p, b2)
        func hue(_ b: Double, _ a: Double) -> Double {
            if a == 0 && b == 0 { return 0 }
            let h = atan2(b, a) * 180 / .pi
            return h < 0 ? h + 360 : h
        }
        let h1p = hue(b1, a1p), h2p = hue(b2, a2p)
        let dLp = l2 - l1, dCp = c2p - c1p
        var dhp = 0.0
        if c1p * c2p != 0 {
            dhp = h2p - h1p
            if dhp > 180 { dhp -= 360 } else if dhp < -180 { dhp += 360 }
        }
        let dHp = 2 * sqrt(c1p * c2p) * sin(dhp * .pi / 360)
        let lbar = (l1 + l2) / 2, cbarp = (c1p + c2p) / 2
        var hbarp = h1p + h2p
        if c1p * c2p != 0 {
            if abs(h1p - h2p) > 180 { hbarp += h1p + h2p < 360 ? 360 : -360 }
            hbarp /= 2
        }
        let t = 1 - 0.17 * cos((hbarp - 30) * .pi / 180) + 0.24 * cos(2 * hbarp * .pi / 180)
            + 0.32 * cos((3 * hbarp + 6) * .pi / 180) - 0.20 * cos((4 * hbarp - 63) * .pi / 180)
        let dTheta = 30 * exp(-pow((hbarp - 275) / 25, 2))
        let rc = 2 * sqrt(pow(cbarp, 7) / (pow(cbarp, 7) + pow(25, 7)))
        let sl = 1 + 0.015 * pow(lbar - 50, 2) / sqrt(20 + pow(lbar - 50, 2))
        let sc = 1 + 0.045 * cbarp, sh = 1 + 0.015 * cbarp * t
        let rt = -sin(2 * dTheta * .pi / 180) * rc
        return sqrt(pow(dLp / sl, 2) + pow(dCp / sc, 2) + pow(dHp / sh, 2) + rt * (dCp / sc) * (dHp / sh))
    }
}
