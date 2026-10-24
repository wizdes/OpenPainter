import AppKit
import COpPresentSpike
import Foundation
import PresenterSpikeKit

// Usage: PresenterSpikeHost [suite] [bench] [probe-hidden-a] [probe-hidden-b]
// Runs on the main thread; every window is 400 x 300 or smaller and closed at the end.
setvbuf(stdout, nil, _IOLBF, 0)
let args = Set(CommandLine.arguments.dropFirst())
let all = args.isEmpty
Host.boot()

// Every probe gets a watchdog so a blocked call cannot leave a process running.
func watchdog(_ seconds: Double, _ label: String) {
    DispatchQueue.global().asyncAfter(deadline: .now() + seconds) {
        print("WATCHDOG: \(label) did not finish within \(seconds) s; exiting with code 3")
        exit(3)
    }
}

do {
    let ctx = try EngineContext()
    if all || args.contains("suite") {
        watchdog(900, "suite")
        for path in ["A", "B"] {
            let t0 = Date()
            let results = try Scenarios.suite(path: path, ctx: ctx)
            for r in results { print(r) }
            print(String(format: "[%@] suite took %.1f s", path, Date().timeIntervalSince(t0)))
        }
    }
    if all || args.contains("bench") {
        watchdog(1200, "bench")
        let doc = Charts.big()
        for path in ["A", "B"] {
            let h: Harness = path == "A" ? try PathA(ctx: ctx) : try PathB(ctx: ctx)
            for scale in [1.0, 2.0] {
                for percent in [100.0, 25.0] {
                    print(try Bench.run(h, doc: doc, scale: scale, percent: percent))
                }
            }
            h.close()
        }
    }
    if args.contains("hideshow") {
        watchdog(300, "hideshow")
        for path in ["A", "B"] {
            let h: Harness = path == "A" ? try PathA(ctx: ctx) : try PathB(ctx: ctx)
            let t0 = Date()
            var r = Scenarios.hideShow(h)
            Scenarios.annotate(&r, h)
            print(r)
            print(String(format: "[%@] hide/show took %.1f s", path, Date().timeIntervalSince(t0)))
            h.close()
        }
    }
    for path in ["A", "B"] where args.contains("probe-hidden-\(path.lowercased())") {
        // Render 10 frames while the window is ordered out.
        let h: Harness = path == "A" ? try PathA(ctx: ctx) : try PathB(ctx: ctx)
        try h.setDocument(Charts.chart(0))
        h.window.orderOut(nil)
        Host.pump(0.05)
        watchdog(20, "render while hidden, path \(path)")
        var ok = 0
        for i in 0..<10 {
            let t0 = Date()
            let c = try h.renderAndCheck(checkColor: false)
            print(String(format: "[%@] hidden frame %d: %@ in %.1f ms %@", path, i, c.ok ? "ok" : "FAIL",
                         Date().timeIntervalSince(t0) * 1000, c.failures.joined(separator: "; ")))
            if c.ok { ok += 1 }
        }
        print("[\(path)] hidden: \(ok)/10 frames rendered without blocking")
        h.close()
    }
    print("final wgpu report: \(try ctx.report())")
} catch {
    print("ERROR: \(error)")
    exit(1)
}
exit(0)
