import AppKit
import COpPresentSpike
import CoreGraphics
import IOSurface

public struct ScenarioResult: CustomStringConvertible {
    public let name: String
    public let path: String
    public var iterations = 0
    public var failures: [String] = []
    public var maxColorError: Double = 0
    public var maxDeltaE: Double = 0
    public var split = ColorSplit()
    public var patchesChecked = 0
    public var bytesChecked = 0
    public var worstFrameMs = 0.0
    public var worstAcquireMs = 0.0
    public var simulated = false
    public var note = ""
    public var passed: Bool { failures.isEmpty }

    mutating func add(_ c: FrameCheck, _ label: String) {
        iterations += 1
        maxColorError = max(maxColorError, c.maxColorError)
        maxDeltaE = max(maxDeltaE, c.maxDeltaE)
        worstFrameMs = max(worstFrameMs, c.totalMs)
        worstAcquireMs = max(worstAcquireMs, c.acquireMs)
        split.merge(c.split)
        patchesChecked += c.patchesChecked
        bytesChecked += c.bytesChecked
        for f in c.failures where failures.count < 20 { failures.append("\(label): \(f)") }
    }

    public var description: String {
        "[\(path)] \(name): \(passed ? "PASS" : "FAIL") n=\(iterations) maxErr=\(String(format: "%.3f", maxColorError)) maxDE00=\(String(format: "%.3f", maxDeltaE)) \(split) "
            + "patches=\(patchesChecked) bytes=\(bytesChecked) worstFrame=\(String(format: "%.1f", worstFrameMs))ms worstAcquire=\(String(format: "%.1f", worstAcquireMs))ms \(note)"
            + (failures.isEmpty ? "" : "\n  " + failures.prefix(8).joined(separator: "\n  "))
    }
}

/// A seeded random source so runs repeat.
struct SplitMix: RandomNumberGenerator {
    var state: UInt64
    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476C_E5E4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
}

public enum Scenarios {
    static func run(_ r: inout ScenarioResult, _ h: Harness, _ label: String, color: Bool = true) {
        do {
            let c = try h.renderAndCheck(checkColor: color)
            r.add(c, label)
        } catch {
            r.iterations += 1
            if r.failures.count < 20 { r.failures.append("\(label): \(error)") }
        }
        Host.pump(0.001)
    }

    static func standardView() -> ViewState {
        var s = ViewState()
        s.widthPt = 160
        s.heightPt = 100
        return s
    }

    /// 200 random sizes from 1x1 to 2000x1500 points, backing scale 1 or 2.
    public static func resizes(_ h: Harness, count: Int = 200, seed: UInt64 = 9) -> ScenarioResult {
        var r = ScenarioResult(name: "200 resizes", path: h.pathName)
        var rng = SplitMix(state: seed)
        do { try h.setDocument(Charts.chart(2)) } catch { r.failures.append("\(error)"); return r }
        var sizes: [(Double, Double, Double)] = [(1, 1, 1), (1, 1, 2), (2000, 1500, 2), (2000, 1500, 1), (1, 1500, 1), (2000, 1, 2), (333, 77, 2)]
        while sizes.count < count {
            let w = Double(Int.random(in: 1...2000, using: &rng))
            let hh = Double(Int.random(in: 1...1500, using: &rng))
            sizes.append((w, hh, Bool.random(using: &rng) ? 2 : 1))
        }
        var odd = 0
        for (i, (w, hh, scale)) in sizes.enumerated() {
            var s = h.state
            s.widthPt = w
            s.heightPt = hh
            s.scale = scale
            if Int(w * scale) % 2 == 1 || Int(hh * scale) % 2 == 1 { odd += 1 }
            do { try h.setView(s) } catch { r.failures.append("resize \(i): \(error)"); continue }
            run(&r, h, "resize \(i) \(Int(w))x\(Int(hh))@\(Int(scale))")
        }
        r.note = "\(odd) odd pixel sizes"
        return r
    }

    /// 100 switches between the sRGB, Display P3 and Adobe RGB charts.
    public static func documentSwitches(_ h: Harness, count: Int = 100) -> ScenarioResult {
        var r = ScenarioResult(name: "100 document switches", path: h.pathName)
        let docs = Charts.all
        do { try h.setView(standardView()) } catch { r.failures.append("\(error)"); return r }
        for i in 0..<count {
            do { try h.setDocument(docs[i % docs.count]) } catch { r.failures.append("switch \(i): \(error)"); continue }
            run(&r, h, "switch \(i) \(docs[i % docs.count].name)")
        }
        return r
    }

    /// orderOut / orderFront 50 times; a frame after every show.
    public static func hideShow(_ h: Harness, count: Int = 50) -> ScenarioResult {
        var r = ScenarioResult(name: "50 hide/show", path: h.pathName)
        do {
            try h.setView(standardView())
            try h.setDocument(Charts.chart(1))
        } catch { r.failures.append("\(error)"); return r }
        for i in 0..<count {
            h.window.orderOut(nil)
            Host.pump(0.01)
            h.window.orderFrontRegardless()
            Host.pump(0.01)
            run(&r, h, "show \(i)")
        }
        return r
    }

    /// Simulated screen moves: didChangeScreen (display profile sRGB -> P3 -> sRGB ...)
    /// and didChangeBackingProperties (scale 1 <-> 2), each followed by a checked frame,
    /// across all three documents.
    public static func screenMoves(_ h: Harness, count: Int = 30) -> ScenarioResult {
        var r = ScenarioResult(name: "\(count) screen moves + \(count) backing changes (simulated)", path: h.pathName)
        r.simulated = true
        let docs = Charts.all
        do { try h.setView(standardView()) } catch { r.failures.append("\(error)"); return r }
        let before = h.events.count
        for i in 0..<count {
            do { try h.setDocument(docs[i % docs.count]) } catch { r.failures.append("\(error)") }
            NotificationCenter.default.post(name: NSWindow.didChangeScreenNotification, object: h.window)
            run(&r, h, "screen \(i)")
            NotificationCenter.default.post(name: NSWindow.didChangeBackingPropertiesNotification, object: h.window)
            run(&r, h, "backing \(i)")
        }
        let seen = h.events.count - before
        if seen < count * 2 { r.failures.append("only \(seen) of \(count * 2) notifications reached the handler") }
        r.note = "displays cycled: " + h.simulatedDisplays.map { ($0.name as String?) ?? "icc" }.joined(separator: ", ")
        return r
    }

    /// Simulated sleep and wake: willSleep drops the drawable pipeline, a frame while
    /// asleep must be refused cleanly, didWake rebuilds it and the next frame is checked.
    public static func sleepWake(_ h: Harness, count: Int = 20) -> ScenarioResult {
        var r = ScenarioResult(name: "\(count) sleep/wake (simulated)", path: h.pathName)
        r.simulated = true
        let docs = Charts.all
        do { try h.setView(standardView()) } catch { r.failures.append("\(error)"); return r }
        let ws = NSWorkspace.shared.notificationCenter
        var refused = 0
        for i in 0..<count {
            do { try h.setDocument(docs[i % docs.count]) } catch { r.failures.append("\(error)") }
            ws.post(name: NSWorkspace.willSleepNotification, object: NSWorkspace.shared)
            var f = OpPsFrame()
            let st = op_ps_render(h.presenter, &f)
            if st == 8 { refused += 1 } else { r.failures.append("sleep \(i): render while asleep returned \(st), want 8") }
            ws.post(name: NSWorkspace.didWakeNotification, object: NSWorkspace.shared)
            run(&r, h, "wake \(i)")
        }
        r.note = "\(refused) frames refused while asleep"
        return r
    }

    /// Create and destroy 100 presenters on one window; live objects must return to baseline.
    public static func retained(ctx: EngineContext, path: String, window: NSWindow, count: Int = 100) -> ScenarioResult {
        var r = ScenarioResult(name: "100 create/destroy", path: path)
        do {
            try ctx.maintain()
            let base = try ctx.report()
            var layers: [WeakObject] = []
            var surfaces: [WeakObject] = []
            var during: OpPsReport?
            for i in 0..<count {
                autoreleasepool {
                    do {
                        let h: Harness = path == "A" ? try PathA(ctx: ctx, window: window) : try PathB(ctx: ctx, window: window)
                        try h.setView(standardView())
                        try h.setDocument(Charts.chart(i % 3))
                        let c = try h.renderAndCheck(checkColor: i % 10 == 0)
                        r.add(c, "presenter \(i)")
                        layers.append(WeakObject(h.view.metalLayer))
                        if let b = h as? PathB {
                            surfaces += b.surfaces.map { WeakObject(unsafeBitCast($0, to: IOSurface.self)) }
                        }
                        if i == 0 { during = try ctx.report() }
                        h.close()
                    } catch {
                        r.failures.append("presenter \(i): \(error)")
                    }
                }
            }
            Host.pump(0.2)
            try ctx.maintain()
            let after = try ctx.report()
            let liveLayers = layers.filter { $0.object != nil }.count
            let heldSurfaces = surfaces.filter { $0.object != nil }.count
            if after.liveCounts != base.liveCounts {
                r.failures.append("live objects changed:\n    before \(base)\n    after  \(after)")
            }
            if liveLayers != 0 { r.failures.append("\(liveLayers) CAMetalLayers still alive") }
            if heldSurfaces != 0 { r.failures.append("\(heldSurfaces) IOSurfaces still alive") }
            if let during, during.live_presenters == 0 || during.wgpu_textures == 0 {
                r.failures.append("counters did not see the live presenter: \(during)")
            }
            r.note = "before {\(base)} with one presenter {\(during.map { "\($0)" } ?? "-")} after {\(after)}; "
                + "layers alive \(liveLayers)/\(layers.count); IOSurfaces alive \(heldSurfaces)/\(surfaces.count)"
        } catch {
            r.failures.append("\(error)")
        }
        return r
    }

    /// Adds the presenter's own counters to the result.
    public static func annotate(_ r: inout ScenarioResult, _ h: Harness) {
        guard let st = try? h.stats() else { return }
        var parts = ["frames \(st.frames)", "validation errors \(st.validation_errors)"]
        if h.pathName == "A" {
            parts += ["configures \(st.configures)", "tag kept by configure \(st.tag_kept_by_configure)",
                      "tag changed by configure \(st.tag_changed_by_configure)", "retags \(st.retags)",
                      "acquire errors \(st.surface_errors)", "layer tag \(cString(st.colorspace_name))"]
            if st.tag_changed_by_configure != 0 { r.failures.append("wgpu configure changed the layer colorspace") }
        } else {
            parts += ["ring violations \(st.ring_violations)"]
            if st.ring_violations != 0 { r.failures.append("ring violations \(st.ring_violations)") }
        }
        r.note += (r.note.isEmpty ? "" : "; ") + parts.joined(separator: ", ")
    }

    /// Runs the whole suite for one path on its own window.
    public static func suite(path: String, ctx: EngineContext) throws -> [ScenarioResult] {
        var out: [ScenarioResult] = []
        func fresh() throws -> Harness { path == "A" ? try PathA(ctx: ctx) : try PathB(ctx: ctx) }
        let steps: [(Harness) -> ScenarioResult] = [
            { resizes($0) }, { documentSwitches($0) }, { hideShow($0) }, { screenMoves($0) }, { sleepWake($0) },
        ]
        for make in steps {
            let h = try fresh()
            var r = make(h)
            annotate(&r, h)
            out.append(r)
            h.close()
        }
        let w = Host.makeWindow(title: "M09 retained-view test")
        out.append(retained(ctx: ctx, path: path, window: w))
        w.orderOut(nil)
        w.close()
        return out
    }
}

final class WeakObject {
    weak var object: AnyObject?
    init(_ o: AnyObject) { object = o }
}
