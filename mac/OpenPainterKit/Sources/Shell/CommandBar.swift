// The command buttons at the start of the tool-options row. The window has no
// toolbar row: the row starts with New, Open, Save | Undo, Redo | Crop to
// Selection, Magic Wand, then More (ellipsis.circle). More opens a menu with
// the rest of the old toolbar: Print, Cut, Copy, Paste, Deselect, Pixel Grid,
// Rulers and the four panel toggles. A button sends its main-menu command's
// action to the object the menu command would reach, and takes its enabled
// state from that object's menu validation. docs/design/SPEC.md, "Toolbar".
import AppKit

/// One command in the bar or the More menu, as the main menu has it.
struct BarCommand {
    let key: String
    let symbol: String
    let title: String
    /// The shortcut in the tooltip ("" for none).
    let shortcut: String
    /// The main menu's action; nil for a command that is not built yet.
    let action: Selector?
    /// The engine command for `runCommand(_:)`.
    var command: String?
    var keyEquivalent = ""
    var modifiers: NSEvent.ModifierFlags = [.command]
    /// New and Open are always on: the app's ImageDocumentController always
    /// enables File > New and Open. (A test host has no document types, so a
    /// plain NSDocumentController would turn them off there.)
    var alwaysEnabled = false

    var tooltip: String { shortcut.isEmpty ? title : "\(title) (\(shortcut))" }
}

extension EditorWindowController {
    /// The seven buttons, left to right, in groups of 3, 2 and 2.
    static let barCommands: [[BarCommand]] = [
        [BarCommand(key: "new", symbol: "doc.badge.plus", title: "New", shortcut: "⌘N",
                    action: #selector(NSDocumentController.newDocument(_:)), alwaysEnabled: true),
         BarCommand(key: "open", symbol: "folder", title: "Open", shortcut: "⌘O",
                    action: #selector(NSDocumentController.openDocument(_:)), alwaysEnabled: true),
         BarCommand(key: "save", symbol: "square.and.arrow.down", title: "Save", shortcut: "⌘S",
                    action: #selector(NSDocument.save(_:)))],
        [BarCommand(key: "undo", symbol: "arrow.uturn.backward", title: "Undo", shortcut: "⌘Z",
                    action: #selector(StandardEditActions.undo(_:))),
         BarCommand(key: "redo", symbol: "arrow.uturn.forward", title: "Redo", shortcut: "⇧⌘Z",
                    action: #selector(StandardEditActions.redo(_:)))],
        [BarCommand(key: "crop", symbol: "crop", title: "Crop to Selection", shortcut: "⇧⌘X",
                    action: #selector(EditorMenuActions.runCommand(_:)), command: "image.crop_to_selection"),
         BarCommand(key: "wand", symbol: PaintTool.magicWand.symbol, title: PaintTool.magicWand.label,
                    shortcut: String(PaintTool.magicWand.key).uppercased(),
                    action: #selector(EditorWindowController.selectMagicWand(_:)))],
    ]

    /// The More menu's commands before the panel toggles; nil is a separator.
    static let moreCommands: [BarCommand?] = [
        BarCommand(key: "print", symbol: "printer", title: "Print", shortcut: "", action: nil),
        nil,
        BarCommand(key: "cut", symbol: "scissors", title: "Cut", shortcut: "⌘X",
                   action: #selector(NSText.cut(_:)), keyEquivalent: "x"),
        BarCommand(key: "copy", symbol: "doc.on.doc", title: "Copy", shortcut: "⌘C",
                   action: #selector(NSText.copy(_:)), keyEquivalent: "c"),
        BarCommand(key: "paste", symbol: "doc.on.clipboard", title: "Paste", shortcut: "⌘V",
                   action: #selector(NSText.paste(_:)), keyEquivalent: "v"),
        nil,
        BarCommand(key: "deselect", symbol: "rectangle.dashed.badge.record", title: "Deselect", shortcut: "⌘D",
                   action: #selector(EditorMenuActions.runCommand(_:)), command: "selection.deselect",
                   keyEquivalent: "d"),
        nil,
        BarCommand(key: "grid", symbol: "grid", title: "Pixel Grid", shortcut: "⌘'",
                   action: #selector(CanvasMenuActions.togglePixelGrid(_:)), keyEquivalent: "'"),
        BarCommand(key: "rulers", symbol: "ruler", title: "Rulers", shortcut: "⌥⌘R",
                   action: #selector(ViewMenuActions.toggleRulers(_:)), keyEquivalent: "r",
                   modifiers: [.command, .option]),
    ]

    /// Builds the buttons and puts them at the start of the options row. Called once.
    func setUpCommandBar() {
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 12, bottom: 0, right: 0)
        var buttons: [ShellButton] = []
        for group in Self.barCommands {
            for c in group {
                let b: ShellButton = c.key == "wand"
                    ? ToggleButton(symbol: c.symbol, label: c.title, tooltip: c.tooltip, target: self,
                                   action: #selector(commandBarClicked(_:)))
                    : ShellButton(symbol: c.symbol, label: c.title, tooltip: c.tooltip, target: self,
                                  action: #selector(commandBarClicked(_:)))
                b.identifier = NSUserInterfaceItemIdentifier(c.key)
                commandButtons[c.key] = b
                buttons.append(b)
                stack.addArrangedSubview(b)
            }
            stack.setCustomSpacing(12, after: buttons[buttons.count - 1])
        }
        stack.setCustomSpacing(6, after: buttons[buttons.count - 1])  // More sits with the seven
        let more = ShellButton(symbol: "ellipsis.circle", label: "More commands", target: self,
                               action: #selector(showMoreMenu(_:)))
        moreButton = more
        buttons.append(more)
        stack.addArrangedSubview(more)
        stack.setCustomSpacing(12, after: more)
        // A 1 pt line between the commands and the tool options.
        let line = FilledView(fill: Theme.line)
        line.translatesAutoresizingMaskIntoConstraints = false
        line.widthAnchor.constraint(equalToConstant: 1).isActive = true
        line.heightAnchor.constraint(equalToConstant: 20).isActive = true
        stack.addArrangedSubview(line)
        commandBarButtons = buttons
        optionsBar.commandBar = stack
        workspace.tools.observe(self) { [weak self] in self?.refreshCommandBar() }
        if let window {
            // AppKit sends this after each event, as it does to validate toolbar items.
            NotificationCenter.default.addObserver(
                forName: NSWindow.didUpdateNotification, object: window, queue: .main
            ) { [weak self] _ in MainActor.assumeIsolated { self?.refreshCommandBar() } }
        }
        refreshCommandBar()
    }

    /// The object that gets `action` from the main menu while this window is
    /// key: the focused view and its responders, the window, this controller,
    /// the document, then the app and the document controller.
    func commandTarget(for action: Selector) -> AnyObject? {
        var r: NSResponder? = window?.firstResponder
        while let x = r {
            if x.responds(to: action) { return x }
            r = x.nextResponder
        }
        let rest: [AnyObject?] = [window, self, document, NSApp, NSApp.delegate, NSDocumentController.shared]
        return rest.compactMap { $0 }.first { $0.responds(to: action) }
    }

    /// A menu item like the main menu's, aimed at the object that would get it.
    /// No target (or a command not built yet) gives an item with no action, which
    /// a menu shows as off.
    func mirrorItem(_ c: BarCommand, image: Bool = true) -> NSMenuItem {
        let item = NSMenuItem(title: c.title, action: nil, keyEquivalent: c.keyEquivalent)
        item.keyEquivalentModifierMask = c.modifiers
        item.representedObject = c.command
        item.toolTip = c.action == nil ? "Comes in a later version" : c.tooltip
        if image { item.image = NSImage(systemSymbolName: c.symbol, accessibilityDescription: nil) }
        if let action = c.action, let target = commandTarget(for: action) {
            item.action = action
            item.target = target
        }
        return item
    }

    /// Enabled states (the main menu's validation) and the Magic Wand's pressed state.
    func refreshCommandBar() {
        guard !commandBarButtons.isEmpty else { return }
        let commands = Self.barCommands.flatMap { $0 }
        let menu = NSMenu()
        menu.autoenablesItems = true
        let items = commands.map { mirrorItem($0, image: false) }
        for i in items { menu.addItem(i) }
        menu.update()
        for (c, item) in zip(commands, items) {
            let on = item.action != nil && (c.alwaysEnabled || item.isEnabled)
            if commandButtons[c.key]?.isEnabled != on { commandButtons[c.key]?.isEnabled = on }
        }
        if let wand = commandButtons["wand"] as? ToggleButton {
            let on = workspace.tools.tool == .magicWand
            if wand.isOn != on || wand.state != (on ? .on : .off) { wand.isOn = on }
            wand.setAccessibilityValue(on ? "selected" : "not selected")
        }
    }

    /// A button: the same action, target and enabled rule as its menu command.
    @objc func commandBarClicked(_ sender: NSButton) {
        defer { refreshCommandBar() }
        guard let key = sender.identifier?.rawValue,
              let c = Self.barCommands.joined().first(where: { $0.key == key }) else { return }
        let item = mirrorItem(c, image: false)
        let menu = NSMenu()
        menu.addItem(item)
        menu.update()
        guard let action = item.action, let target = item.target, c.alwaysEnabled || item.isEnabled else { return }
        sendCommand(action, target, item)
    }

    /// Magic Wand button: picks the tool, as its Tools panel button does.
    @objc func selectMagicWand(_ sender: Any?) {
        workspace.tools.tool = .magicWand
    }

    /// The More menu, built fresh so its items validate against the current state.
    public func moreMenu() -> NSMenu {
        let menu = NSMenu(title: "More commands")
        menu.autoenablesItems = true
        for c in Self.moreCommands {
            guard let c else {
                menu.addItem(.separator())
                continue
            }
            menu.addItem(mirrorItem(c))
        }
        menu.addItem(.separator())
        for k in PanelKind.allCases {
            let item = NSMenuItem(title: k.title, action: #selector(PanelMenuTarget.togglePanel(_:)),
                                  keyEquivalent: String(Character(UnicodeScalar(UInt16(NSF5FunctionKey - 5 + k.functionKey))!)))
            item.keyEquivalentModifierMask = []
            item.target = workspace.menuTarget
            item.tag = k.digit
            item.toolTip = "\(k.title) (F\(k.functionKey) or ⌥⌘\(k.digit))"
            item.image = NSImage(systemSymbolName: k.symbol, accessibilityDescription: nil)
            menu.addItem(item)
        }
        L10n.localize(menu, context: "More commands menu")
        return menu
    }

    @objc func showMoreMenu(_ sender: NSButton) {
        let menu = moreMenu()
        let below = sender.isFlipped ? sender.bounds.maxY + 4 : -4
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: below), in: sender)
    }
}
