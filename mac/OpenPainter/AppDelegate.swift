// App start-up: menus, command-line arguments, the empty start window, and the
// `--trace <png>` performance gate.
import AppKit
import OpenPainterKit

/// Counts documents that are still being read, so the start window is not shown
/// while a file from Finder, the Dock or the command line is opening.
final class PainterDocumentController: NSDocumentController {
    private(set) var opening = 0

    override func openDocument(
        withContentsOf url: URL, display displayDocument: Bool,
        completionHandler: @escaping (NSDocument?, Bool, (any Error)?) -> Void
    ) {
        opening += 1
        super.openDocument(withContentsOf: url, display: displayDocument) { [weak self] doc, shown, error in
            self?.opening -= 1
            completionHandler(doc, shown, error)
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var startWindow: NSWindow?
    private var tracePath: String?
    private var trace: TraceRunner?

    func applicationWillFinishLaunching(_ notification: Notification) {
        _ = PainterDocumentController()  // becomes NSDocumentController.shared
        PainterDocument.prepareEngine()
        let workspace = Workspace(engine: AppEngine.shared, tools: ToolState.shared)
        Workspace.shared = workspace
        workspace.startEvents()
        NSApp.mainMenu = MainMenu.build(workspace)
        NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
        ) { _ in MainActor.assumeIsolated { Workspace.shared?.panels.screensChanged() } }
        NSApp.setActivationPolicy(.regular)
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    func applicationDidFinishLaunching(_ notification: Notification) {
        var args = Array(CommandLine.arguments.dropFirst())
        var files: [String] = []
        while !args.isEmpty {
            let a = args.removeFirst()
            if a == "--trace", !args.isEmpty {
                tracePath = args.removeFirst()
            } else if a == "--make-trace-png", !args.isEmpty {
                let out = URL(fileURLWithPath: args.removeFirst())
                do {
                    try TraceImage.write(to: out)
                    print("wrote \(out.path)")
                    exit(0)
                } catch {
                    FileHandle.standardError.write(Data("could not write \(out.path): \(error)\n".utf8))
                    exit(2)
                }
            } else if a.hasPrefix("-") {
                // AppKit's own arguments (-NSDocumentRevisionsDebugMode YES and so on).
                if !args.isEmpty, !args[0].hasPrefix("-") { args.removeFirst() }
            } else {
                files.append(a)
            }
        }
        if let tracePath {
            runTrace(tracePath)
            return
        }
        for f in files { open(URL(fileURLWithPath: f)) }
        DispatchQueue.main.async { self.showStartWindowIfIdle() }
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls { open(url) }
    }

    private func open(_ url: URL) {
        NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { [weak self] _, _, error in
            if let error {
                FileHandle.standardError.write(Data("OpenPainter: could not open \(url.path): \(error.localizedDescription)\n".utf8))
                NSApp.presentError(error)
                self?.showStartWindowIfIdle()
            } else {
                self?.startWindow?.close()
                self?.startWindow = nil
            }
        }
    }

    /// With no document open or opening, show a small empty window and report ready.
    private func showStartWindowIfIdle() {
        let controller = NSDocumentController.shared as? PainterDocumentController
        guard NSDocumentController.shared.documents.isEmpty, (controller?.opening ?? 0) == 0, startWindow == nil
        else { return }
        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 240),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        w.title = "OpenPainter"
        w.isRestorable = false
        w.isReleasedWhenClosed = false
        let label = NSTextField(labelWithString: "Open an image with File > Open (⌘O),\nor drop a PNG on the Dock icon.")
        label.alignment = .center
        label.textColor = .secondaryLabelColor
        label.frame = NSRect(x: 20, y: 90, width: 380, height: 60)
        w.contentView?.addSubview(label)
        w.center()
        w.makeKeyAndOrderFront(nil)
        startWindow = w
        w.displayIfNeeded()
        CATransaction.flush()
        LaunchClock.markReady()
    }

    // MARK: --trace

    private func runTrace(_ path: String) {
        let url = URL(fileURLWithPath: path)
        NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { [weak self] doc, _, error in
            MainActor.assumeIsolated { self?.startTrace(doc, error, path) }
        }
    }

    private func startTrace(_ doc: NSDocument?, _ error: (any Error)?, _ path: String) {
        guard let doc = doc as? PainterDocument,
            let wc = doc.windowControllers.first as? EditorWindowController
        else {
            FileHandle.standardError.write(Data("TRACE could not open \(path): \(String(describing: error))\n".utf8))
            exit(2)
        }
        wc.window?.setContentSize(NSSize(width: 960, height: 600))
        let canvas = wc.container.canvas
        canvas.onFirstFrame = { [weak self] in
            LaunchClock.markReady()
            let runner = TraceRunner(canvas: canvas)
            self?.trace = runner
            runner.run { result in
                FileHandle.standardOutput.write(Data((result.description + "\n").utf8))
                doc.close()
                exit(result.passed ? 0 : 1)
            }
        }
    }
}
