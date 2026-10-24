import AppKit
import COpPresentSpike
import CoreGraphics
import IOSurface
import Metal
import QuartzCore

/// A layer-backed view whose backing layer is a CAMetalLayer.
public final class MetalView: NSView {
    public override func makeBackingLayer() -> CALayer { CAMetalLayer() }
    public var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
    public override var isFlipped: Bool { true }
}

public enum Host {
    /// Makes sure AppKit is ready, without taking focus from the user.
    public static func boot() {
        _ = NSApplication.shared
        if NSApp.activationPolicy() != .accessory { NSApp.setActivationPolicy(.accessory) }
    }

    public static func pump(_ seconds: TimeInterval = 0.005) {
        CATransaction.flush()
        RunLoop.current.run(until: Date(timeIntervalSinceNow: seconds))
    }

    /// A small window (400 x 300) in the bottom-left corner of the main screen.
    public static func makeWindow(title: String) -> NSWindow {
        boot()
        let screen = NSScreen.main!.visibleFrame
        let window = NSWindow(contentRect: NSRect(x: screen.minX + 8, y: screen.minY + 8, width: 400, height: 300),
                              styleMask: [.titled, .miniaturizable], backing: .buffered, defer: false)
        window.title = title
        window.isReleasedWhenClosed = false
        window.contentView = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        window.contentView!.wantsLayer = true
        window.orderFrontRegardless()
        pump(0.05)
        return window
    }

    public static func screenICC(_ window: NSWindow) -> Data? {
        (window.screen ?? NSScreen.main)?.colorSpace?.cgColorSpace?.copyICCData() as Data?
    }
}

public struct FrameCheck {
    public var failures: [String] = []
    public var renderMs = 0.0
    public var acquireMs = 0.0
    public var presentMs = 0.0
    /// Path B: GPU time of the Swift blit from the IOSurface into the drawable.
    public var blitGpuMs = 0.0
    public var totalMs = 0.0
    /// Path A: worst patch difference from the ColorSync reference, in 1/255 levels.
    public var maxColorError: Double = 0
    /// Path A: worst CIEDE2000 between the drawable and the ColorSync reference.
    public var maxDeltaE: Double = 0
    public var split = ColorSplit()
    public var patchesChecked = 0
    /// Path B: document bytes compared exactly.
    public var bytesChecked = 0
    public var ok: Bool { failures.isEmpty }
}

public struct ViewState: Equatable {
    public var widthPt: Double = 120
    public var heightPt: Double = 80
    public var scale: Double = 1
    public var zoom: Double = 1
    public var originX: Double = 0
    public var originY: Double = 0
    public var pixels: (Int, Int) {
        (Swift.max(1, Swift.min(16384, Int((widthPt * scale).rounded()))),
         Swift.max(1, Swift.min(16384, Int((heightPt * scale).rounded()))))
    }
    public static func == (a: ViewState, b: ViewState) -> Bool {
        a.widthPt == b.widthPt && a.heightPt == b.heightPt && a.scale == b.scale && a.zoom == b.zoom
            && a.originX == b.originX && a.originY == b.originY
    }
}

/// What both paths share: the window, the view, the Rust presenter, and the
/// notification handlers that simulate screen moves and sleep.
public class Harness {
    public let ctx: EngineContext
    public let window: NSWindow
    public let ownsWindow: Bool
    public let view: MetalView
    public var presenter: OpaquePointer!
    public private(set) var doc: TestDocument?
    public var display: CGColorSpace
    public var state = ViewState()
    public var pathName: String { "?" }
    public var simulatedDisplays: [CGColorSpace] = [CGColorSpace(name: CGColorSpace.sRGB)!,
                                                    CGColorSpace(name: CGColorSpace.displayP3)!]
    var displayCursor = 0
    var observers: [(NotificationCenter, NSObjectProtocol)] = []
    public var events: [String] = []
    var baselineErrors: UInt64 = 0

    init(ctx: EngineContext, window: NSWindow?) throws {
        self.ctx = ctx
        ownsWindow = window == nil
        self.window = window ?? Host.makeWindow(title: "M09 presenter spike")
        view = MetalView(frame: NSRect(x: 0, y: 0, width: 120, height: 80))
        view.wantsLayer = true
        self.window.contentView!.addSubview(view)
        let screenICC = Host.screenICC(self.window)
        display = screenICC.flatMap { CGColorSpace(iccData: $0 as CFData) } ?? CGColorSpace(name: CGColorSpace.sRGB)!
        baselineErrors = try ctx.report().validation_errors
        installObservers()
    }

    func installObservers() {
        let nc = NotificationCenter.default
        observers.append((nc, nc.addObserver(forName: NSWindow.didChangeScreenNotification, object: window, queue: nil) { [weak self] _ in
            self?.events.append("screen")
            try? self?.screenChanged()
        }))
        observers.append((nc, nc.addObserver(forName: NSWindow.didChangeBackingPropertiesNotification, object: window, queue: nil) { [weak self] _ in
            self?.events.append("backing")
            try? self?.backingChanged()
        }))
        let ws = NSWorkspace.shared.notificationCenter
        observers.append((ws, ws.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: nil) { [weak self] _ in
            self?.events.append("sleep")
            try? self?.willSleep()
        }))
        observers.append((ws, ws.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: nil) { [weak self] _ in
            self?.events.append("wake")
            try? self?.didWake()
        }))
    }

    /// Simulated move to another screen: the target display profile changes.
    func screenChanged() throws {
        displayCursor = (displayCursor + 1) % simulatedDisplays.count
        try setDisplay(simulatedDisplays[displayCursor])
    }

    /// Simulated backing change: scale flips between 1 and 2.
    func backingChanged() throws {
        var s = state
        s.scale = s.scale == 1 ? 2 : 1
        try setView(s)
    }

    func willSleep() throws { try check(op_ps_suspend(presenter), "suspend") }
    func didWake() throws { try check(op_ps_resume(presenter), "resume") }

    public func setDocument(_ d: TestDocument) throws {
        doc = d
        _ = try d.rgba.withUnsafeBufferPointer { px in
            try d.icc.withUnsafeBytes { icc in
                try check(op_ps_set_document(presenter, px.baseAddress, px.count, UInt32(d.width), UInt32(d.height),
                                             icc.bindMemory(to: UInt8.self).baseAddress, icc.count), "set_document")
            }
        }
    }

    public func setDisplay(_ space: CGColorSpace) throws { display = space }

    public func setView(_ s: ViewState) throws {
        state = s
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        view.setFrameSize(NSSize(width: s.widthPt, height: s.heightPt))
        view.metalLayer.contentsScale = s.scale
        CATransaction.commit()
        var v = OpPsView(width_pt: s.widthPt, height_pt: s.heightPt, scale: s.scale, zoom: s.zoom,
                         origin_x: s.originX, origin_y: s.originY)
        try check(op_ps_set_view(presenter, &v), "set_view")
    }

    public func stats() throws -> OpPsStats {
        var s = OpPsStats()
        try check(op_ps_stats(presenter, &s), "stats")
        return s
    }

    /// Renders one frame, presents it and checks this path's invariants.
    public func renderAndCheck(checkColor: Bool) throws -> FrameCheck { fatalError("override") }

    func commonChecks(_ c: inout FrameCheck, _ st: OpPsStats) {
        if st.validation_errors != baselineErrors {
            c.failures.append("validation errors \(st.validation_errors - baselineErrors): \(cString(st.last_error))")
            baselineErrors = st.validation_errors
        }
    }

    public func close() {
        for (nc, o) in observers { nc.removeObserver(o) }
        observers.removeAll()
        if presenter != nil {
            op_ps_free(presenter)
            presenter = nil
        }
        view.removeFromSuperview()
        if ownsWindow {
            window.orderOut(nil)
            window.close()
        }
        Host.pump()
    }

    /// The visible document rectangle in target pixels at zoom 1.
    func visibleDocRect(_ d: TestDocument) -> (Int, Int, Int, Int)? {
        let (pw, ph) = state.pixels
        guard state.zoom == 1, state.originX == state.originX.rounded(), state.originY == state.originY.rounded() else { return nil }
        let ox = Int(state.originX), oy = Int(state.originY)
        let x0 = Swift.max(0, ox), y0 = Swift.max(0, oy)
        let x1 = Swift.min(pw, ox + d.width), y1 = Swift.min(ph, oy + d.height)
        return x1 > x0 && y1 > y0 ? (x0, y0, x1, y1) : nil
    }
}

func now() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1e6 }

func iccEqual(_ space: CGColorSpace?, _ icc: Data?) -> Bool {
    guard let space, let icc, let got = space.copyICCData() as Data? else { return false }
    return got == icc
}

// MARK: - Path A

/// Path A: wgpu renders into the view's CAMetalLayer and converts document -> display
/// with the 33^3 table; the layer is tagged with the display profile.
public final class PathA: Harness {
    public override var pathName: String { "A" }
    public var capture = true

    public override init(ctx: EngineContext, window: NSWindow? = nil) throws {
        try super.init(ctx: ctx, window: window)
        var out: OpaquePointer?
        try check(op_ps_new_layer(ctx.raw, Unmanaged.passUnretained(view.metalLayer).toOpaque(), &out), "new_layer")
        presenter = out
        try setDisplay(display)
        try setView(state)
    }

    public override func setDisplay(_ space: CGColorSpace) throws {
        try super.setDisplay(space)
        let icc = space.copyICCData()! as Data
        _ = try icc.withUnsafeBytes { b in
            try check(op_ps_set_display_profile(presenter, b.bindMemory(to: UInt8.self).baseAddress, b.count), "set_display_profile")
        }
    }

    public override func renderAndCheck(checkColor: Bool) throws -> FrameCheck {
        var c = FrameCheck()
        try check(op_ps_set_capture(presenter, checkColor), "set_capture")
        var f = OpPsFrame()
        let t0 = now()
        let status = op_ps_render(presenter, &f)
        c.totalMs = now() - t0
        if status != 0 {
            var buf = [CChar](repeating: 0, count: 512)
            _ = op_ps_last_error(&buf, buf.count)
            c.failures.append("render status \(status): \(String(cString: buf))")
            return c
        }
        c.renderMs = f.render_ms
        c.acquireMs = f.acquire_ms
        let st = try stats()
        commonChecks(&c, st)
        let layer = view.metalLayer
        let (pw, ph) = state.pixels
        // Read the layer back in Swift too, independently of the Rust readback.
        if Int(layer.drawableSize.width) != pw || Int(layer.drawableSize.height) != ph {
            c.failures.append("drawableSize \(layer.drawableSize) != \(pw)x\(ph)")
        }
        if Int(f.width) != pw || Int(f.height) != ph { c.failures.append("frame size \(f.width)x\(f.height) != \(pw)x\(ph)") }
        let displayICC = display.copyICCData()! as Data
        if !iccEqual(layer.colorspace, displayICC) { c.failures.append("layer colorspace is not the display profile") }
        if st.layer_tag_is_display != 1 { c.failures.append("Rust readback: layer tag is not the display profile") }
        if st.tag_changed_by_configure != 0 { c.failures.append("wgpu configure changed the colorspace \(st.tag_changed_by_configure)x") }
        if layer.pixelFormat != .bgra8Unorm { c.failures.append("pixelFormat \(layer.pixelFormat.rawValue)") }
        if checkColor, let doc { try checkColors(doc, &c) }
        return c
    }

    /// Pass rule for path A: every patch within ColorTolerance.deltaE00 of the ColorSync
    /// reference. The level difference is recorded too.
    func checkColors(_ doc: TestDocument, _ c: inout FrameCheck) throws {
        let (pw, ph) = state.pixels
        var bytes = [UInt8](repeating: 0, count: pw * ph * 4)
        var w: UInt32 = 0, h: UInt32 = 0
        try check(op_ps_copy_capture(presenter, &bytes, bytes.count, &w, &h), "copy_capture")
        guard Int(w) == pw, Int(h) == ph else {
            c.failures.append("capture size \(w)x\(h)")
            return
        }
        guard let _ = visibleDocRect(doc) else { return }
        let want = Reference.expected(doc, display: display)
        let split = try ColorSplit.analyse(doc, display: display)
        let ox = Int(state.originX), oy = Int(state.originY)
        for (i, p) in doc.patches.enumerated() {
            let x = ox + p.centerX, y = oy + p.centerY
            guard x >= 0, y >= 0, x < pw, y < ph else { continue }
            let o = (y * pw + x) * 4
            let got = [Double(bytes[o + 2]), Double(bytes[o + 1]), Double(bytes[o])] // BGRA
            var worst = 0.0
            for k in 0..<3 {
                worst = Swift.max(worst, abs(got[k] - Double(want[i][k]) * 255))
                c.split.gpuVsTable = Swift.max(c.split.gpuVsTable, abs(got[k] - Double(split.table[i][k]) * 255))
            }
            c.maxColorError = Swift.max(c.maxColorError, worst)
            let de = DeltaE.de00(display, got.map { $0 / 255 }, want[i].map(Double.init))
            c.maxDeltaE = Swift.max(c.maxDeltaE, de)
            if de > ColorTolerance.deltaE00 {
                c.failures.append(String(format: "patch %d %@: dE00 %.2f, levels off %.1f (%@ -> display)", i, "\(p.rgb)", de, worst, doc.name))
            }
            c.patchesChecked += 1
        }
        c.split.merge(split.summary)
    }
}

// MARK: - Path B

/// Path B: wgpu renders into a ring of 3 IOSurfaces; this Swift-owned CAMetalLayer,
/// tagged with the document profile, presents them (Core Animation converts).
public final class PathB: Harness {
    public override var pathName: String { "B" }
    let device: MTLDevice
    let queue: MTLCommandQueue
    public private(set) var surfaces: [IOSurfaceRef] = []
    var textures: [MTLTexture] = []
    var ringSize: (Int, Int) = (0, 0)
    var iccByGeneration: [UInt64: Data] = [:]
    public var ringCount = 3
    public var checkBytes = true

    public override init(ctx: EngineContext, window: NSWindow? = nil) throws {
        device = MTLCreateSystemDefaultDevice()!
        queue = device.makeCommandQueue()!
        try super.init(ctx: ctx, window: window)
        let layer = view.metalLayer
        layer.device = device
        layer.pixelFormat = .bgra8Unorm
        layer.framebufferOnly = false
        layer.maximumDrawableCount = 3
        layer.isOpaque = true
        var out: OpaquePointer?
        try check(op_ps_new_ring(ctx.raw, &out), "new_ring")
        presenter = out
        try setView(state)
    }

    static func makeSurface(_ w: Int, _ h: Int) -> IOSurfaceRef {
        let props: [CFString: Any] = [
            kIOSurfaceWidth: w, kIOSurfaceHeight: h, kIOSurfaceBytesPerElement: 4,
            kIOSurfaceBytesPerRow: IOSurfaceAlignProperty(kIOSurfaceBytesPerRow, w * 4),
            kIOSurfacePixelFormat: 0x4247_5241, // 'BGRA'
        ]
        return IOSurfaceCreate(props as CFDictionary)!
    }

    /// Builds a new ring at the view's pixel size and hands it to the engine.
    func rebuildRing() throws {
        let (pw, ph) = state.pixels
        let new = (0..<ringCount).map { _ in PathB.makeSurface(pw, ph) }
        var ptrs: [UnsafeMutableRawPointer?] = new.map { Unmanaged.passUnretained($0).toOpaque() }
        _ = try ptrs.withUnsafeMutableBufferPointer { b in
            try check(op_ps_set_ring(presenter, b.baseAddress, UInt32(b.count)), "set_ring")
        }
        surfaces = new
        textures = new.map { s in
            let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: pw, height: ph, mipmapped: false)
            d.usage = [.shaderRead]
            d.storageMode = .shared
            return device.makeTexture(descriptor: d, iosurface: s, plane: 0)!
        }
        ringSize = (pw, ph)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        view.metalLayer.drawableSize = CGSize(width: pw, height: ph)
        CATransaction.commit()
    }

    public override func setView(_ s: ViewState) throws {
        try super.setView(s)
        if state.pixels != ringSize { try rebuildRing() }
    }

    public override func setDocument(_ d: TestDocument) throws {
        try super.setDocument(d)
        iccByGeneration[try stats().doc_generation] = d.icc
    }

    override func willSleep() throws {
        try super.willSleep()
        surfaces = []
        textures = []
        ringSize = (0, 0)
    }

    override func didWake() throws {
        try super.didWake()
        try rebuildRing()
    }

    public override func renderAndCheck(checkColor: Bool) throws -> FrameCheck {
        var c = FrameCheck()
        let t0 = now()
        var f = OpPsFrame()
        try check(op_ps_render(presenter, &f), "render")
        var ready = OpPsFrame()
        try check(op_ps_ring_acquire(presenter, true, &ready), "ring_acquire")
        let t1 = now()
        c.renderMs = ready.render_ms
        if ready.seq != f.seq { c.failures.append("acquired frame \(ready.seq), rendered \(f.seq)") }
        let slot = Int(ready.slot)
        let surface = surfaces[slot]
        if Unmanaged.passUnretained(surface).toOpaque() != ready.iosurface { c.failures.append("slot surface mismatch") }
        if checkBytes, let doc { checkSurfaceBytes(surface, doc, &c) }
        // Tag the layer with the profile of the document this frame shows, in the same
        // transaction as the frame.
        guard let icc = iccByGeneration[ready.doc_generation] else {
            c.failures.append("no ICC for generation \(ready.doc_generation)")
            try check(op_ps_ring_release(presenter, ready.slot), "ring_release")
            return c
        }
        let layer = view.metalLayer
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        if !iccEqual(layer.colorspace, icc) { layer.colorspace = CGColorSpace(iccData: icc as CFData) }
        guard let drawable = layer.nextDrawable() else {
            CATransaction.commit()
            c.failures.append("nextDrawable returned nil")
            try check(op_ps_ring_release(presenter, ready.slot), "ring_release")
            return c
        }
        let cb = queue.makeCommandBuffer()!
        let blit = cb.makeBlitCommandEncoder()!
        let (pw, ph) = state.pixels
        blit.copy(from: textures[slot], sourceSlice: 0, sourceLevel: 0, sourceOrigin: MTLOrigin(x: 0, y: 0, z: 0),
                  sourceSize: MTLSize(width: pw, height: ph, depth: 1), to: drawable.texture, destinationSlice: 0,
                  destinationLevel: 0, destinationOrigin: MTLOrigin(x: 0, y: 0, z: 0))
        blit.endEncoding()
        cb.present(drawable)
        cb.commit()
        CATransaction.commit()
        // Explicit completion: the slot goes back to the engine only after our blit is done.
        cb.waitUntilCompleted()
        c.blitGpuMs = (cb.gpuEndTime - cb.gpuStartTime) * 1000
        try check(op_ps_ring_release(presenter, ready.slot), "ring_release")
        c.presentMs = now() - t1
        c.totalMs = now() - t0
        let st = try stats()
        commonChecks(&c, st)
        if st.ring_violations != 0 { c.failures.append("ring violations \(st.ring_violations)") }
        if Int(layer.drawableSize.width) != pw || Int(layer.drawableSize.height) != ph {
            c.failures.append("drawableSize \(layer.drawableSize) != \(pw)x\(ph)")
        }
        if IOSurfaceGetWidth(surface) != pw || IOSurfaceGetHeight(surface) != ph { c.failures.append("IOSurface size") }
        if let doc, !iccEqual(layer.colorspace, doc.icc) { c.failures.append("layer colorspace is not the document profile") }
        if drawable.texture.width != pw || drawable.texture.height != ph { c.failures.append("drawable texture size") }
        return c
    }

    /// The IOSurface must hold the document bytes exactly (BGRA vs RGBA) at 100%.
    func checkSurfaceBytes(_ s: IOSurfaceRef, _ doc: TestDocument, _ c: inout FrameCheck) {
        guard let (x0, y0, x1, y1) = visibleDocRect(doc) else { return }
        IOSurfaceLock(s, .readOnly, nil)
        defer { IOSurfaceUnlock(s, .readOnly, nil) }
        let base = IOSurfaceGetBaseAddress(s).assumingMemoryBound(to: UInt8.self)
        let row = IOSurfaceGetBytesPerRow(s)
        let ox = Int(state.originX), oy = Int(state.originY)
        var bad = 0
        doc.rgba.withUnsafeBufferPointer { src in
            for y in y0..<y1 {
                for x in x0..<x1 {
                    let o = y * row + x * 4
                    let d = ((y - oy) * doc.width + (x - ox)) * 4
                    if base[o] != src[d + 2] || base[o + 1] != src[d + 1] || base[o + 2] != src[d] || base[o + 3] != 255 {
                        bad += 1
                    }
                }
            }
        }
        c.bytesChecked += (x1 - x0) * (y1 - y0)
        if bad > 0 { c.failures.append("\(bad) IOSurface pixels differ from the document") }
    }

    public override func close() {
        super.close()
        surfaces = []
        textures = []
    }
}
