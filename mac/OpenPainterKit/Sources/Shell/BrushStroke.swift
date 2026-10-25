// The main window's brush drags (M38): the canvas sends pen samples here, and a
// `LiveStroke` sends them to the engine while the pointer moves. The tool
// options in force at the press apply to the whole stroke.
import AppKit

@MainActor
public final class BrushStrokeController: StrokeSink {
    public let live: LiveStroke
    private weak var controller: EditorWindowController?

    init(controller: EditorWindowController) {
        self.controller = controller
        live = LiveStroke(document: controller.editor.document)
        live.onError = { [weak controller] message in controller?.editor.report(message) }
        live.onFinished = { [weak controller] in controller?.editor.refresh() }
    }

    public func strokeBegan(_ sample: PenSample, secondary: Bool) -> Bool {
        guard let controller else { return false }
        let tools = controller.workspace.tools
        guard tools.tool.strokeName != nil else { return false }
        return live.begin(tools.liveStrokeParams(tool: tools.tool, secondary: secondary), first: sample)
    }

    public func strokeMoved(_ sample: PenSample) { live.add(sample) }
    public func strokeEnded() { live.end() }
    public func strokeCancelled() { live.cancel() }
}
