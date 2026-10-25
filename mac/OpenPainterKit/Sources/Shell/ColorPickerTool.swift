// The Color Picker tool (M36), following paint.net's Color Picker page
// (https://www.getpaint.net/doc/latest/ColorPicker.html): a click puts the
// sampled color in the primary slot, a right-click (or Control-click) in the
// secondary slot. The sample is the average of a square window clipped at the
// canvas edges (engine query `doc.sample_color`). Sampling is Layer or Image;
// paint.net's Ctrl-click for Image is Command-click here. After the click the
// tool stays, goes back to the previous tool, or switches to the Pencil.
import AppKit

extension EditorWindowController {
    /// Samples at a document point and applies the After-click choice.
    func pickColor(at point: CGPoint, secondary: Bool, modifiers: NSEvent.ModifierFlags) {
        let tools = workspace.tools
        let x = Int(point.x.rounded(.down)), y = Int(point.y.rounded(.down))
        guard x >= 0, y >= 0, x < editor.info.width, y < editor.info.height else { return }
        var sampling = tools.settings.pickerSampling
        if modifiers.contains(.command) { sampling = .image }
        guard let c = sampleColor(x: x, y: y, size: tools.settings.pickerSize, sampling: sampling) else { return }
        tools.set(secondary ? .secondary : .primary, c)
        switch tools.settings.pickerAfterClick {
        case .doNotSwitch: break
        case .previousTool:
            if let prev = tools.previousTool, prev != .colorPicker { tools.tool = prev }
        case .pencil: tools.tool = .pencil
        }
    }

    /// The averaged color of the sample window, or nil (reported) on failure.
    public func sampleColor(x: Int, y: Int, size: PickerSampleSize, sampling: ToolSettings.Sampling) -> DocColor? {
        do {
            let r = try editor.document.query(
                "doc.sample_color", ["x": x, "y": y, "size": size.rawValue, "sampling": sampling.rawValue])
            guard let hex = (r as? [String: Any])?["color"] as? String, let c = DocColor(hex: hex) else {
                editor.report("Color Picker: the engine gave no color")
                return nil
            }
            return c
        } catch {
            editor.report("Color Picker failed: \(error)")
            return nil
        }
    }

    /// paint.net keys on the canvas: X swaps the colors, C moves the active notch.
    func colorKey(_ c: Character) -> Bool {
        switch c {
        case "x": workspace.tools.swapColors()
        case "c": workspace.tools.toggleActiveSlot()
        default: return false
        }
        return true
    }
}
