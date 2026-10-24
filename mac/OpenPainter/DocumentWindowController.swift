// One window per document: the canvas, plus a tiny titlebar strip with the
// temporary tool switcher and the zoom percent (M15 builds the real panels).
import AppKit
import OpenPainterKit

final class DocumentWindowController: NSWindowController, NSWindowDelegate {
    let container: CanvasContainerView
    private let tools = NSSegmentedControl(labels: ["Pan (H)", "Zoom (Z)"], trackingMode: .selectOne, target: nil, action: nil)
    private let zoomLabel = NSTextField(labelWithString: "100%")

    init(document: Document, pixelSize: CGSize) {
        container = CanvasContainerView(document: document, documentSize: pixelSize)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1000, height: 700),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.contentView = container
        window.isRestorable = false
        window.tabbingMode = .disallowed
        window.minSize = NSSize(width: 320, height: 240)
        super.init(window: window)
        window.delegate = self
        window.cascadeTopLeft(from: NSPoint(x: 40, y: (NSScreen.main?.visibleFrame.maxY ?? 800) - 40))
        window.initialFirstResponder = container.canvas
        setUpStrip(window)
        container.onViewportChange = { [weak self] v in self?.zoomLabel.stringValue = ZoomPresets.label(v.zoom) }
        container.canvas.onFirstFrame = { LaunchClock.markReady() }
        container.canvas.onToolChange = { [weak self] tool in self?.tools.selectedSegment = tool.rawValue }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func setUpStrip(_ window: NSWindow) {
        tools.selectedSegment = 0
        tools.target = self
        tools.action = #selector(toolPicked(_:))
        tools.controlSize = .small
        zoomLabel.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        zoomLabel.alignment = .right
        let stack = NSStackView(views: [tools, zoomLabel])
        stack.orientation = .horizontal
        stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 12)
        stack.frame = NSRect(x: 0, y: 0, width: 260, height: 28)
        let accessory = NSTitlebarAccessoryViewController()
        accessory.view = stack
        accessory.layoutAttribute = .trailing
        window.addTitlebarAccessoryViewController(accessory)
    }

    @objc private func toolPicked(_ sender: NSSegmentedControl) {
        container.canvas.tool = CanvasTool(rawValue: sender.selectedSegment) ?? .pan
        window?.makeFirstResponder(container.canvas)
    }

    func detachCanvas() {
        container.canvas.detach()
    }

    func windowWillClose(_ notification: Notification) {
        detachCanvas()
    }
}
