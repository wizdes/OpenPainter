// The History panel (M20): one 27 pt row per step with its name and an icon
// for its command. The current step has a 2 pt edge and a diamond. Undone
// (future) steps stay readable in `muted` text with a redo arrow on the right.
// A click jumps to that step. Undo and Redo sit in the action row.
import AppKit

/// Icons and display names for history steps.
public enum HistoryStep {
    /// The SF Symbol for a step's command id.
    public static func symbol(_ command: String) -> String {
        switch command {
        case "document.open": return "photo"
        case "layer.add": return "plus.square"
        case "layer.delete": return "trash"
        case "layer.duplicate": return "plus.square.on.square"
        case "layer.merge_down": return "arrow.down.to.line"
        case "layer.move_up": return "arrow.up"
        case "layer.move_down": return "arrow.down"
        case "layer.move": return "arrow.up.arrow.down"
        case "layer.set_visible": return "eye"
        case "layer.properties": return "slider.horizontal.3"
        case "selection.select_all", "selection.deselect", "move.selection": return "rectangle.dashed"
        // Live strokes (M38) name their step after the tool.
        case "tool.stroke", "Paintbrush", "Eraser", "Pencil": return "paintbrush.pointed"
        case "tool.magic_wand": return "wand.and.stars"
        case "tool.paint_bucket": return "drop"
        case "image.crop_to_selection", "image.auto_crop": return "crop"
        case "image.flip_horizontal": return "arrow.left.and.right"
        case "image.flip_vertical": return "arrow.up.and.down"
        case "image.rotate_90_cw", "image.rotate_90_ccw", "image.rotate_180": return "rotate.right"
        case "image.flatten": return "square.3.layers.3d.down.right"
        case "image.resize", "image.canvas_size": return "arrow.up.left.and.arrow.down.right"
        case "image.assign_profile", "image.convert_profile": return "paintpalette"
        default:
            if command.hasPrefix("effect.adjust.") || command == "image.invert_colors" { return "circle.lefthalf.filled" }
            if command.hasPrefix("effect.") { return "sparkles" }
            if command.hasPrefix("move") || command.hasPrefix("Move") { return "arrow.up.and.down.and.arrow.left.and.right" }
            return "circle.fill"
        }
    }

    /// The engine labels a few steps with their command id; show paint.net's
    /// names for those.
    public static func title(_ label: String) -> String {
        switch label {
        case "layer.add": return "Add New Layer"
        case "layer.delete": return "Delete Layer"
        default: return label
        }
    }
}

/// A history row: the command icon on the left; undone rows add a redo arrow
/// where the current row has its diamond.
public final class HistoryRow: PanelRow {
    public let undone: Bool
    let redoMark = NSImageView()

    init(index: Int, label: String, command: String, undone: Bool) {
        self.undone = undone
        super.init(index: index, title: HistoryStep.title(label), detail: nil, symbol: HistoryStep.symbol(command),
                   height: 27)
        dimmed = undone
        icon.contentTintColor = undone ? Theme.muted : Theme.ink
        if undone {
            let config = NSImage.SymbolConfiguration(pointSize: 11, weight: .regular)
            redoMark.image = NSImage(systemSymbolName: "arrow.uturn.forward", accessibilityDescription: "undone")?
                .withSymbolConfiguration(config)
            redoMark.contentTintColor = Theme.muted
            addSubview(redoMark)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override func layout() {
        super.layout()
        redoMark.frame = NSRect(x: bounds.width - 20, y: (bounds.height - 14) / 2, width: 14, height: 14)
    }
}

@MainActor
public final class HistoryPanelView: RowListPanelView {
    weak var workspace: Workspace?
    public private(set) var undoButton: NSButton!
    public private(set) var redoButton: NSButton!
    public var historyRows: [HistoryRow] { rows.compactMap { $0 as? HistoryRow } }

    init(workspace: Workspace) {
        self.workspace = workspace
        super.init(rowHeight: 27)
        undoButton = NSButton(title: "Undo", image: NSImage(systemSymbolName: "arrow.uturn.backward", accessibilityDescription: nil)!,
                              target: self, action: #selector(undo(_:)))
        redoButton = NSButton(title: "Redo", image: NSImage(systemSymbolName: "arrow.uturn.forward", accessibilityDescription: nil)!,
                              target: self, action: #selector(redo(_:)))
        redoButton.imagePosition = .imageTrailing
        undoButton.toolTip = "Undo (⌘Z)"
        redoButton.toolTip = "Redo (⇧⌘Z)"
        for b in [undoButton!, redoButton!] {
            b.isBordered = false
            b.font = Theme.font(12)
            b.contentTintColor = Theme.ink
            b.heightAnchor.constraint(equalToConstant: 24).isActive = true
            b.widthAnchor.constraint(greaterThanOrEqualToConstant: 24).isActive = true
        }
        let spacer = NSView()
        spacer.setContentHuggingPriority(.init(1), for: .horizontal)
        actions.addArrangedSubview(undoButton)
        actions.addArrangedSubview(spacer)
        actions.addArrangedSubview(redoButton)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    var busy: Bool { FocusRouter.target(in: workspace?.active?.windowController?.window) == .dialog }

    func reload() {
        guard let e = workspace?.active else {
            setRows([])
            undoButton.isEnabled = false
            redoButton.isEnabled = false
            return
        }
        let current = e.info.historyIndex
        let commands = e.info.historyCommands
        let rows = e.info.historyLabels.enumerated().map { i, label in
            let undone = i > current
            let row = HistoryRow(index: i, label: label, command: i < commands.count ? commands[i] : "",
                                 undone: undone)
            row.isCurrent = i == current
            row.setAccessibilityValue(undone ? "undone" : (i == current ? "current" : "done"))
            row.onClick = { [weak self] index in self?.jump(index) }
            return row
        }
        setRows(rows)
        undoButton.isEnabled = !busy && current > 0
        redoButton.isEnabled = !busy && current + 1 < e.info.historyLabels.count
        scrollToCurrent()
    }

    func jump(_ index: Int) {
        guard !busy, let e = workspace?.active, index != e.info.historyIndex else { return }
        e.run("history.jump", ["index": index])
    }

    @objc func undo(_ sender: Any?) { if !busy { workspace?.active?.run("edit.undo") } }
    @objc func redo(_ sender: Any?) { if !busy { workspace?.active?.run("edit.redo") } }
}
