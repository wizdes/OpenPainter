// Layers > Remove Background (M53): Select Foreground and Remove Background.
//
// paint.net 5.1 has no Remove Background command: its Image menu page
// (https://www.getpaint.net/doc/latest/ImageMenu.html) and Layers menu page
// (https://www.getpaint.net/doc/latest/LayersMenu.html) list none. The item
// sits in the Layers menu because it works on the active layer, and that page
// says its commands "only affect the active layer".
//
// - The mask is computed from the active layer's own pixels (`layer.pixels`),
//   not the flattened image, at full document size, by Apple Vision
//   (`PlatformServices/ForegroundMask.swift`, which calls mac/VisionMask), off
//   the main thread.
// - A sheet shows an indeterminate progress bar and Cancel while Vision runs.
//   Cancel stops Vision and sends nothing, so the document stays as it was.
// - Then one engine command, so one history step:
//   Select Foreground -> `selection.from_mask` (a Coverage selection holding
//   the soft mask); Remove Background -> `layer.remove_background` (the active
//   layer's alpha times the mask). The selection is not used by either.
// - No foreground found: a status message, nothing sent. If the image changed
//   while Vision ran (another window's command), nothing is sent either.
import AppKit

/// Layers menu selectors the document window controller implements (M53).
@MainActor @objc public protocol RemoveBackgroundActions {
    func selectForeground(_ sender: Any?)
    func removeBackground(_ sender: Any?)
}

/// What the mask is used for.
public enum RemoveBackgroundOutcome: String, Sendable {
    case selectForeground = "selection.from_mask"
    case removeBackground = "layer.remove_background"

    public var title: String { self == .selectForeground ? "Select Foreground" : "Remove Background" }
}

/// Where the mask comes from. Tests swap `compute` to hold a run open (Cancel
/// tests) or to give a fixed mask; the default is Vision.
public enum ForegroundMasker {
    public typealias Compute = @Sendable (ForegroundMaskJob, [UInt8], Int, Int) throws -> [UInt8]?
    public static let vision: Compute = { job, rgba, w, h in try job.run(rgba: rgba, width: w, height: h) }
    nonisolated(unsafe) public static var compute: Compute = vision
}

/// The progress sheet and the run of one Remove Background item.
@MainActor
public final class RemoveBackgroundController: NSWindowController {
    public enum State: Equatable {
        case running
        /// The engine command was sent with this ticket.
        case applied(UInt64)
        case cancelled
        case noForeground
        case stale
        case failed(String)
    }

    public let editor: Editor
    public let outcome: RemoveBackgroundOutcome
    public private(set) var state: State = .running
    public let progress = NSProgressIndicator()
    public let messageLabel = Labels.label("Finding the foreground…", size: 12, color: Theme.ink)
    public private(set) var cancelButton: NSButton!
    let job = ForegroundMaskJob()
    private var startRevision = -1
    /// Called once when the run ends, in any state (tests wait on it).
    public var onFinish: ((State) -> Void)?

    /// The open run for each editor (one at a time).
    static var open: [ObjectIdentifier: RemoveBackgroundController] = [:]
    public static func current(for editor: Editor) -> RemoveBackgroundController? { open[ObjectIdentifier(editor)] }

    static let width: CGFloat = 360

    public init(editor: Editor, outcome: RemoveBackgroundOutcome) {
        self.editor = editor
        self.outcome = outcome
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: 116), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = outcome.title
        super.init(window: panel)
        build(panel)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func build(_ panel: NSPanel) {
        let content = FilledView(fill: Theme.panel)
        panel.contentView = content
        // y grows down (FilledView is flipped). 16 pt margins, 12 pt gaps.
        messageLabel.frame = NSRect(x: 16, y: 20, width: Self.width - 32, height: 16)
        content.addSubview(messageLabel)
        progress.style = .bar
        progress.isIndeterminate = true
        progress.controlSize = .small
        progress.frame = NSRect(x: 16, y: 46, width: Self.width - 32, height: 12)
        progress.setAccessibilityLabel("Finding the foreground")
        content.addSubview(progress)
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        cancelButton.frame = NSRect(x: Self.width - 16 - 80, y: 72, width: 80, height: 28)
        content.addSubview(cancelButton)
    }

    /// Reads the active layer, shows the sheet and starts Vision. False (and a
    /// status message) when the layer cannot be read.
    @discardableResult
    public func start(on window: NSWindow) -> Bool {
        let doc = editor.document
        let rgba: [UInt8]
        let (w, h): (Int, Int)
        do {
            let info = try doc.info()
            startRevision = (info["revision"] as? NSNumber)?.intValue ?? -1
            w = (info["width"] as? NSNumber)?.intValue ?? 0
            h = (info["height"] as? NSNumber)?.intValue ?? 0
            rgba = [UInt8](try doc.queryData("layer.pixels"))
        } catch {
            editor.report("\(outcome.title): could not read the layer: \(error)")
            return false
        }
        Self.open[ObjectIdentifier(editor)] = self
        L10n.localizeDialog(self.window!)  // M59: translated and named before it shows
        window.beginSheet(self.window!) { _ in }
        progress.startAnimation(nil)
        let job = self.job
        let compute = ForegroundMasker.compute
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result = Result { try compute(job, rgba, w, h) }
            DispatchQueue.main.async { self?.finish(result) }
        }
        return true
    }

    /// Cancel (or Escape): stops Vision; nothing is sent to the engine.
    @objc public func cancel(_ sender: Any?) {
        guard state == .running else { return }
        job.cancel()
        end(.cancelled)
    }

    private func finish(_ result: Result<[UInt8]?, Error>) {
        guard state == .running else { return }  // Cancel already ended it.
        switch result {
        case let .failure(error):
            if error as? ForegroundMaskError == .cancelled { return end(.cancelled) }
            editor.report("\(outcome.title) failed: \(error)")
            end(.failed("\(error)"))
        case let .success(mask):
            guard let mask, mask.contains(where: { $0 != 0 }) else {
                editor.report("\(outcome.title): no foreground found")
                return end(.noForeground)
            }
            let now = ((try? editor.document.info())?["revision"] as? NSNumber)?.intValue ?? -2
            guard now == startRevision else {
                editor.report("\(outcome.title): the image changed while it ran; nothing was done")
                return end(.stale)
            }
            guard let ticket = editor.run(outcome.rawValue, ["mask": Data(mask).base64EncodedString()]) else {
                return end(.failed("the engine refused the command"))
            }
            end(.applied(ticket))
        }
    }

    private func end(_ s: State) {
        state = s
        progress.stopAnimation(nil)
        if let sheet = window, let parent = sheet.sheetParent { parent.endSheet(sheet) }
        Self.open[ObjectIdentifier(editor)] = nil
        onFinish?(s)
    }
}

extension EditorWindowController: RemoveBackgroundActions {
    @objc public func selectForeground(_ sender: Any?) { startRemoveBackground(.selectForeground) }
    @objc public func removeBackground(_ sender: Any?) { startRemoveBackground(.removeBackground) }

    /// Starts one Remove Background run with its progress sheet. Nil when a
    /// sheet is already up or the layer cannot be read.
    @discardableResult
    public func startRemoveBackground(_ outcome: RemoveBackgroundOutcome) -> RemoveBackgroundController? {
        guard let window, window.attachedSheet == nil, RemoveBackgroundController.current(for: editor) == nil else {
            return nil
        }
        let c = RemoveBackgroundController(editor: editor, outcome: outcome)
        return c.start(on: window) ? c : nil
    }
}
