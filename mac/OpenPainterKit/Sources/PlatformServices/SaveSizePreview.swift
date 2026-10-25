// M50: the save sheet's live file-size preview. Each change of options asks
// for an encode at those options on a background queue; the size (and, for
// lossy formats, the decoded image) comes back on the main thread.
//
// Stale work is cancelled in two ways. A request that has not started yet is
// replaced by the newer one, so only the latest waits. An encode that is
// already running cannot be stopped inside the codec, so its result is
// dropped when a newer request exists. Only the result of the last request
// is ever shown, as paint.net's Save Configuration dialog does.
import CoreGraphics
import Foundation
import ImageIO

public final class SaveSizePreview: @unchecked Sendable {
    public struct Result: @unchecked Sendable {
        /// Which request this answers (1 for the first).
        public var generation: Int
        public var options: SaveOptions
        /// The encoded size in bytes, nil on failure.
        public var bytes: Int?
        /// The encoded file decoded again, for lossy formats.
        public var image: CGImage?
        public var error: String?
    }

    public typealias Encode = @Sendable (SaveOptions) throws -> Data

    private let encode: Encode
    private let decodeImage: Bool
    private let lock = NSLock()
    private var generation = 0
    private var pending: (Int, SaveOptions)?
    private var running = false
    private var started = 0
    private let queue = DispatchQueue(label: "OpenPainter.save-size-preview", qos: .userInitiated)
    /// Called on the main thread with the latest request's result only.
    public var onResult: (@MainActor (Result) -> Void)?

    /// `decodeImage` true decodes each encoded file for the preview image.
    public init(decodeImage: Bool, encode: @escaping Encode) {
        self.decodeImage = decodeImage
        self.encode = encode
    }

    /// How many encodes ran (tests: skipped stale requests never start).
    public var encodesStarted: Int { lock.withLock { started } }
    /// The newest request's number.
    public var latestGeneration: Int { lock.withLock { generation } }

    /// Asks for the size at `options`; older unfinished requests go stale.
    @discardableResult
    public func request(_ options: SaveOptions) -> Int {
        lock.lock()
        generation += 1
        let g = generation
        pending = (g, options)
        let start = !running
        running = true
        lock.unlock()
        if start { queue.async { [weak self] in self?.drain() } }
        return g
    }

    /// Drops every pending and running request (the sheet closed).
    public func cancel() {
        lock.withLock {
            generation += 1
            pending = nil
        }
    }

    private func isCurrent(_ g: Int) -> Bool { lock.withLock { g == generation } }

    private func drain() {
        while true {
            lock.lock()
            guard let (g, options) = pending else {
                running = false
                lock.unlock()
                return
            }
            pending = nil
            started += 1
            lock.unlock()
            var result = Result(generation: g, options: options)
            do {
                let data = try encode(options)
                result.bytes = data.count
                if decodeImage, isCurrent(g) { result.image = Self.decode(data) }
            } catch {
                result.error = (error as? LocalizedError)?.errorDescription ?? "\(error)"
            }
            guard isCurrent(g) else { continue }  // stale: a newer request exists
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated {
                    guard let self, self.isCurrent(g) else { return }
                    self.onResult?(result)
                }
            }
        }
    }

    /// The encoded file as an image no larger than 1024 pixels on a side.
    static func decode(_ data: Data) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceThumbnailMaxPixelSize: 1024,
            kCGImageSourceCreateThumbnailWithTransform: true,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, opts as CFDictionary)
    }
}

extension SaveSizePreview.Result {
    init(generation: Int, options: SaveOptions) {
        self.init(generation: generation, options: options, bytes: nil, image: nil, error: nil)
    }
}

/// "123 KB" style sizes for the sheet (base 1000, as Finder shows them).
public enum FileSizeText {
    public static func text(_ bytes: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }
}
