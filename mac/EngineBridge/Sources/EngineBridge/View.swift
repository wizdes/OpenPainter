// Canvas views: a document drawn by the engine into a CAMetalLayer (M12).
import COpenPainter
import Foundation
import QuartzCore

/// What the engine needs to draw one view. Sizes and positions are in points.
public struct CanvasViewState: Equatable, Sendable {
    public var widthPt: Double
    public var heightPt: Double
    /// Backing scale factor of the window's screen.
    public var scale: Double
    /// Points per document pixel: 1.0 is 100%.
    public var zoom: Double
    /// Position of the document's top-left corner from the view's top-left corner
    /// (y grows down).
    public var originX: Double
    public var originY: Double
    public var pixelGrid: Bool
    /// True while the Mac sleeps: the engine drops the drawable surface.
    public var suspended: Bool

    public init(
        widthPt: Double, heightPt: Double, scale: Double, zoom: Double,
        originX: Double, originY: Double, pixelGrid: Bool = false, suspended: Bool = false
    ) {
        self.widthPt = widthPt
        self.heightPt = heightPt
        self.scale = scale
        self.zoom = zoom
        self.originX = originX
        self.originY = originY
        self.pixelGrid = pixelGrid
        self.suspended = suspended
    }

    var flags: UInt32 {
        (pixelGrid ? UInt32(OP_VIEW_PIXEL_GRID) : 0) | (suspended ? UInt32(OP_VIEW_SUSPENDED) : 0)
    }
}

/// A view's state and statistics as the engine holds them.
public struct CanvasViewInfo: Sendable {
    public let state: CanvasViewState
    public let doc: UInt64
    public let layerTagIsDisplay: Bool
    public let hasDisplayTable: Bool
    public let suspended: Bool
    public let displayICCLength: Int
    public let frames: UInt64
    public let drawableWidth: Int
    public let drawableHeight: Int
    public let configures: UInt64
    public let tagChangedByConfigure: UInt64
    public let tableBuilds: UInt64
    public let mipBuilds: UInt64
    public let docUploads: UInt64
    public let frameErrors: UInt64
    public let lastError: EngineError?
    /// Changes uploaded as changed tiles only (live strokes), and those tiles.
    public let regionUploads: UInt64
    public let regionTiles: UInt64
    /// The fill around the document, red, green, blue in 0...1 (M56).
    public let background: [Double]

    init(_ i: OpViewInfo) {
        state = CanvasViewState(
            widthPt: i.width_pt, heightPt: i.height_pt, scale: i.scale, zoom: i.zoom,
            originX: i.origin_x, originY: i.origin_y,
            pixelGrid: i.flags & UInt32(OP_VIEW_PIXEL_GRID) != 0,
            suspended: i.flags & UInt32(OP_VIEW_SUSPENDED) != 0)
        doc = i.doc
        layerTagIsDisplay = i.layer_tag_is_display != 0
        hasDisplayTable = i.has_display_table != 0
        suspended = i.suspended != 0
        displayICCLength = Int(i.display_icc_len)
        frames = i.frames
        drawableWidth = Int(i.drawable_width)
        drawableHeight = Int(i.drawable_height)
        configures = i.configures
        tagChangedByConfigure = i.tag_changed_by_configure
        tableBuilds = i.table_builds
        mipBuilds = i.mip_builds
        docUploads = i.doc_uploads
        frameErrors = i.frame_errors
        lastError = EngineError(OpStatus(rawValue: i.last_error))
        regionUploads = i.region_uploads
        regionTiles = i.region_tiles
        background = [Double(i.background_red), Double(i.background_green), Double(i.background_blue)]
    }
}

/// Engine-wide view counters, for leak checks.
public struct CanvasViewCounters: Equatable, Sendable {
    public let liveViews: UInt64
    public let liveLayers: UInt64
    public let liveSurfaces: UInt64
    public let wgpuTextures: UInt64
    public let wgpuTextureViews: UInt64
    public let wgpuBuffers: UInt64
    public let wgpuBindGroups: UInt64
    public let validationErrors: UInt64
    public let framesTotal: UInt64

    /// Everything except the frame count, which only grows.
    public var resources: [UInt64] {
        [liveViews, liveLayers, liveSurfaces, wgpuTextures, wgpuTextureViews, wgpuBuffers, wgpuBindGroups]
    }
}

extension Engine {
    public func viewCounters() throws -> CanvasViewCounters {
        var c = OpViewCounters()
        try withHandle { h in try checkStatus(op_view_counters(h, &c)) }
        return CanvasViewCounters(
            liveViews: c.live_views, liveLayers: c.live_layers, liveSurfaces: c.live_surfaces,
            wgpuTextures: c.wgpu_textures, wgpuTextureViews: c.wgpu_texture_views,
            wgpuBuffers: c.wgpu_buffers, wgpuBindGroups: c.wgpu_bind_groups,
            validationErrors: c.validation_errors, framesTotal: c.frames_total)
    }
}

func checkStatus(_ status: OpStatus) throws {
    if let error = EngineError(status) { throw error }
}

/// One canvas view: the engine draws `document` into `layer` on its render thread.
/// The engine retains the layer until `detach()`.
public final class EngineView: @unchecked Sendable {
    public let document: Document
    public let id: UInt64
    private let lock = NSLock()
    private var attached = true

    public init(document: Document, layer: CAMetalLayer) throws {
        self.document = document
        var id: UInt64 = 0
        let raw = Unmanaged.passUnretained(layer).toOpaque()
        try document.engine.withHandle { h in try checkStatus(op_view_attach(h, document.id, raw, &id)) }
        self.id = id
    }

    deinit { try? detach() }

    /// Sends the newest state. `displayICC` is the screen's ICC profile; pass nil
    /// to keep the one sent before. Returns at once; the engine draws a frame soon.
    public func set(_ state: CanvasViewState, displayICC: Data? = nil) throws {
        try document.engine.withHandle { h in
            try withICC(displayICC) { ptr, len in
                var s = OpViewState(
                    width_pt: state.widthPt, height_pt: state.heightPt, scale: state.scale,
                    zoom: state.zoom, origin_x: state.originX, origin_y: state.originY,
                    display_icc: ptr, display_icc_len: len, flags: state.flags)
                try checkStatus(op_view_set(h, id, &s))
            }
        }
    }

    /// Sets the fill around the document (`op_view_set_background`, M56).
    /// Components are 0 to 1 in the display's encoding.
    public func setBackground(red: Double, green: Double, blue: Double) throws {
        try document.engine.withHandle { h in try checkStatus(op_view_set_background(h, id, red, green, blue)) }
    }

    private func withICC(_ icc: Data?, _ body: (UnsafePointer<UInt8>?, UInt) throws -> Void) throws {
        guard let icc, !icc.isEmpty else { return try body(nil, 0) }
        try icc.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, UInt(raw.count))
        }
    }

    public func info() throws -> CanvasViewInfo {
        var i = OpViewInfo()
        try document.engine.withHandle { h in try checkStatus(op_view_info(h, id, &i)) }
        return CanvasViewInfo(i)
    }

    /// Moves the present timestamps (ms since attach, oldest first) out of the engine.
    public func takeFrameTimes() throws -> [Double] {
        var all: [Double] = []
        var chunk = [Double](repeating: 0, count: 4096)
        while true {
            var n: UInt = 0
            try document.engine.withHandle { h in
                try chunk.withUnsafeMutableBufferPointer { buf in
                    try checkStatus(op_view_take_frame_times(h, id, buf.baseAddress, UInt(buf.count), &n))
                }
            }
            all.append(contentsOf: chunk[0..<Int(n)])
            if Int(n) < chunk.count { return all }
        }
    }

    /// Blocks until the view has presented `frames` frames. Returns false on timeout.
    /// It waits in short slices so other engine calls are not held up.
    public func waitFrames(_ frames: UInt64, timeout: Duration) throws -> Bool {
        let deadline = ContinuousClock.now + timeout
        while true {
            var status = OP_OK
            try document.engine.withHandle { h in
                status = op_view_wait_frames(h, id, frames, 10, nil)
            }
            if status == OP_OK { return true }
            if status != OP_ERR_BUSY { try checkStatus(status) }
            if ContinuousClock.now >= deadline { return false }
        }
    }

    /// Waits for the in-flight frame, then releases the layer. Safe to call twice.
    public func detach() throws {
        lock.lock()
        defer { lock.unlock() }
        guard attached else { return }
        attached = false
        do {
            try document.engine.withHandle { h in try checkStatus(op_view_detach(h, id)) }
        } catch EngineError.closed, EngineError.noSuchDoc {
            // The engine or the document is gone; it already released the layer.
        }
    }
}
