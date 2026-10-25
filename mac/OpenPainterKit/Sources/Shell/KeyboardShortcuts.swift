// Keyboard use (M59): the keys that are not menu items, the panel focus
// cycle, and the switch that makes custom controls reachable with Tab.
//
// Sources: paint.net's Keyboard & Mouse Commands page
// (https://www.getpaint.net/doc/latest/KeyboardMouseCommands.html, revised
// 3 August 2024) for the keys; Apple's Mac keyboard shortcuts page
// (https://support.apple.com/en-us/102650) for the system keys we must not
// take. docs/shortcuts.md is generated from this table and the menu catalog
// (tests/catalog/paintnet-5.1-menus.toml) by KeyboardAccessibilityTests.
import AppKit

/// Whether custom controls (layer and history rows, swatches, the wheel, the
/// color sliders) take keyboard focus. They follow the system's Keyboard
/// navigation setting (System Settings > Keyboard), as AppKit's own buttons
/// and popups do (`NSApplication.isFullKeyboardAccessEnabled`). Tests set
/// `override` to check the key view loop without changing the setting.
@MainActor
public enum KeyboardAccess {
    public static var override: Bool?
    public static var enabled: Bool { override ?? NSApp.isFullKeyboardAccessEnabled }

    /// Space or Return (keypad Enter too), with no Command, Control or Option.
    static func isPress(_ event: NSEvent) -> Bool {
        guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return false }
        return [49, 36, 76].contains(event.keyCode)
    }

    /// -1 for the up arrow, 1 for the down arrow, nil otherwise.
    static func verticalStep(_ event: NSEvent) -> Int? {
        switch event.keyCode {
        case 126: -1
        case 125: 1
        default: nil
        }
    }
}

/// One key that is not a menu item.
public struct KeyShortcut: Sendable, Equatable {
    /// What it does.
    public let action: String
    /// paint.net's Windows key ("" for none).
    public let win: String
    /// OpenPainter's Mac key, in the menu notation (⌃⌥⇧⌘ then the key).
    public let mac: String
    /// Where it works: "Canvas", "Any window", "Panel row", ...
    public let scope: String
    public let note: String

    public init(_ action: String, win: String, mac: String, scope: String, note: String = "") {
        self.action = action
        self.win = win
        self.mac = mac
        self.scope = scope
        self.note = note
    }
}

/// A macOS system shortcut (Apple's Mac keyboard shortcuts page and System
/// Settings > Keyboard > Keyboard Shortcuts, defaults on macOS 15 to 27).
public struct SystemShortcut: Sendable {
    public let key: String
    public let what: String
    /// True when the standard Mac app menu item of the same key is ours to have
    /// (Hide, Quit, Minimize, Close, Settings): not a clash.
    public let standardAppItem: Bool
}

public enum KeyboardShortcuts {
    /// Keys that work on the canvas or in panels and are not menu items. Tool
    /// letters come from `PaintTool.key`; repeated presses cycle the tools
    /// that share a letter, and Shift with the letter cycles backwards
    /// (paint.net: "Shift + S" is Magic Wand, the last S tool).
    public static var canvas: [KeyShortcut] {
        var out: [KeyShortcut] = []
        for letter in uniqueToolKeys {
            let group = PaintTool.allCases.filter { $0.key == letter }
            let L = String(letter).uppercased()
            for (i, t) in group.enumerated() {
                let presses = i == 0 ? L : "\(L) ×\(i + 1)"
                let back = group.count > 1 ? "⇧\(L)" + (group.count - i > 1 ? " ×\(group.count - i)" : "") : ""
                out.append(KeyShortcut(
                    t.label, win: group.count > 1 ? "\(presses)\(back.isEmpty ? "" : " or Shift+\(L)\(group.count - i > 1 ? " x\(group.count - i)" : "")")" : L,
                    mac: back.isEmpty ? presses : "\(presses) or \(back)", scope: "Canvas",
                    note: t.isFunctional ? (group.count > 1 ? "Shared letter: each press picks the next tool" : "")
                        : "The tool is in the panel but not built yet"))
            }
        }
        out += [
            KeyShortcut("Brush width −1", win: "[", mac: "[", scope: "Canvas"),
            KeyShortcut("Brush width +1", win: "]", mac: "]", scope: "Canvas"),
            KeyShortcut("Brush width −5", win: "Ctrl+[", mac: "⌘[", scope: "Canvas"),
            KeyShortcut("Brush width +5", win: "Ctrl+]", mac: "⌘]", scope: "Canvas"),
            KeyShortcut("Swap primary and secondary colors", win: "X", mac: "X", scope: "Canvas"),
            KeyShortcut("Switch the active color (the notch)", win: "C", mac: "C", scope: "Canvas"),
            KeyShortcut("Pan while held", win: "Space", mac: "Space", scope: "Canvas", note: "Hold and drag"),
            KeyShortcut("Nudge the Move tools 1 px", win: "Arrows", mac: "Arrows", scope: "Canvas"),
            KeyShortcut("Nudge the Move tools 10 px", win: "Ctrl+Arrows", mac: "⌘Arrows", scope: "Canvas"),
            KeyShortcut("Cancel a drag, the crop, a move or a fill; finish text; else Deselect", win: "Esc",
                        mac: "Esc", scope: "Canvas", note: "mac/README.md, Focus rules"),
            KeyShortcut("Finish the crop, a move or a fill; else Deselect", win: "Enter", mac: "↩", scope: "Canvas"),
            KeyShortcut("Next image", win: "Ctrl+Tab or Ctrl+PgDn", mac: "⌃Tab", scope: "Canvas",
                        note: "⌘` also moves between image windows (macOS)"),
            KeyShortcut("Previous image", win: "Ctrl+Shift+Tab or Ctrl+PgUp", mac: "⌃⇧Tab", scope: "Canvas"),
            KeyShortcut("Next or previous control", win: "Tab, Shift+Tab", mac: "Tab, ⇧Tab", scope: "Any window",
                        note: "Buttons and rows take focus when Keyboard navigation is on (System Settings > Keyboard)"),
            KeyShortcut("Pick the focused row, swatch or button", win: "", mac: "Space or ↩", scope: "Panels"),
            KeyShortcut("Row above or below", win: "", mac: "↑ ↓", scope: "Layers and History rows"),
            KeyShortcut("Palette swatch to the left, right, above or below", win: "", mac: "← → ↑ ↓",
                        scope: "Palette"),
            KeyShortcut("Hue −1° / +1° (⇧: 15°); saturation −1 / +1 (⇧: 10)", win: "", mac: "← → ↓ ↑",
                        scope: "Color wheel"),
            KeyShortcut("Default button; Cancel", win: "Enter; Esc", mac: "↩; Esc", scope: "Dialogs"),
        ]
        return out
    }

    /// The tool letters in Tools-panel order, each once.
    static var uniqueToolKeys: [Character] {
        var seen: [Character] = []
        for t in PaintTool.allCases where !seen.contains(t.key) { seen.append(t.key) }
        return seen
    }

    /// paint.net keys OpenPainter does not have in this version, and why.
    public static let notInThisVersion: [(win: String, action: String, reason: String)] = [
        ("Alt, Alt + letter", "Menu access keys", "Mac menus have no access letters; ⌃F2 moves focus to the menu bar (macOS)"),
        ("Alt+T", "Open the tool dropdown", "The options row's tool popup is reached with Tab"),
        ("PgUp, PgDn, Home, End (and Shift, Ctrl)", "Scroll the canvas", "Follow-up; scroll with the trackpad or the Pan tool"),
        ("Space + Arrows", "Pan with the keys", "Follow-up"),
        ("Ctrl+Shift+F5 to F8", "Reset one window", "Window > Reset Panel Layout resets all four"),
        ("Ctrl+1 to 9, Alt+1 to 9", "Switch to image number N", "Follow-up; ⌥⌘1 to 4 toggle the panels"),
        ("Ctrl+Shift+PgUp, PgDn", "Move the image tab left or right", "Follow-up; drag in the image strip"),
        ("Alt + −", "Image context menu", "No image context menu in this version"),
        ("A, Shift+A", "Cycle shapes", "The Shapes tool is not built yet"),
        (", . /", "Line / Curve caps and style", "The Line / Curve tool is not built yet"),
        ("Ctrl+Alt+Shift+~", "Diagnostic aid", "Windows only"),
        ("F1", "Online documentation", "Help > OpenPainter Help (⇧⌘?) is off until the help pages exist"),
    ]

    /// macOS shortcuts that apps should not take. Sources: Apple's "Mac keyboard
    /// shortcuts" (https://support.apple.com/en-us/102650) and the defaults in
    /// System Settings > Keyboard > Keyboard Shortcuts.
    public static let system: [SystemShortcut] = [
        .init(key: "⌘Space", what: "Spotlight", standardAppItem: false),
        .init(key: "⌥⌘Space", what: "Spotlight in Finder", standardAppItem: false),
        .init(key: "⌃Space", what: "Previous input source", standardAppItem: false),
        .init(key: "⌃⌥Space", what: "Next input source", standardAppItem: false),
        .init(key: "⌃⌘Space", what: "Emoji & Symbols", standardAppItem: false),
        .init(key: "⌘Tab", what: "Switch apps", standardAppItem: false),
        .init(key: "⌘`", what: "Next window of the app", standardAppItem: false),
        .init(key: "⇧⌘`", what: "Previous window of the app", standardAppItem: false),
        .init(key: "⇧⌘3", what: "Screenshot", standardAppItem: false),
        .init(key: "⇧⌘4", what: "Screenshot of a part", standardAppItem: false),
        .init(key: "⇧⌘5", what: "Screenshot and recording", standardAppItem: false),
        .init(key: "⌃⌘Q", what: "Lock Screen", standardAppItem: false),
        .init(key: "⇧⌘Q", what: "Log Out", standardAppItem: false),
        .init(key: "⌥⌘⎋", what: "Force Quit", standardAppItem: false),
        .init(key: "⌥⌘D", what: "Show or hide the Dock", standardAppItem: false),
        .init(key: "⌃⌘F", what: "Full screen", standardAppItem: false),
        .init(key: "⌘F5", what: "VoiceOver on or off", standardAppItem: false),
        .init(key: "⌥⌘F5", what: "Accessibility Shortcuts", standardAppItem: false),
        .init(key: "⌃F1", what: "Keyboard navigation on or off", standardAppItem: false),
        .init(key: "⌃F2", what: "Focus the menu bar", standardAppItem: false),
        .init(key: "⌃F3", what: "Focus the Dock", standardAppItem: false),
        .init(key: "⌃F4", what: "Focus the next window", standardAppItem: false),
        .init(key: "⌃F5", what: "Focus the window toolbar", standardAppItem: false),
        .init(key: "⌃F6", what: "Focus the floating window", standardAppItem: false),
        .init(key: "⌃F7", what: "Change the way Tab moves focus", standardAppItem: false),
        .init(key: "⌃F8", what: "Focus the status menus", standardAppItem: false),
        .init(key: "⌃↑", what: "Mission Control", standardAppItem: false),
        .init(key: "⌃↓", what: "Application windows", standardAppItem: false),
        .init(key: "⌃←", what: "Move left a space", standardAppItem: false),
        .init(key: "⌃→", what: "Move right a space", standardAppItem: false),
        .init(key: "⇧⌘/", what: "Help menu search", standardAppItem: true),
        .init(key: "⌘H", what: "Hide the app", standardAppItem: true),
        .init(key: "⌥⌘H", what: "Hide others", standardAppItem: true),
        .init(key: "⌘M", what: "Minimize", standardAppItem: true),
        .init(key: "⌘Q", what: "Quit", standardAppItem: true),
        .init(key: "⌘W", what: "Close", standardAppItem: true),
        .init(key: "⌘,", what: "Settings", standardAppItem: true),
    ]

    /// Notes on keys that work differently from paint.net on a Mac keyboard.
    public static let macNotes: [String] = [
        "F5 to F8 send brightness, dictation or media keys on Apple keyboards unless fn is held or "
            + "\"Use F1, F2, etc. keys as standard function keys\" is on. ⌥⌘1 to ⌥⌘4 toggle the same panels.",
        "Buttons, popups and custom controls take focus with Tab only while Keyboard navigation is on "
            + "(System Settings > Keyboard; ⌃F7 toggles it). Text fields always take focus.",
        "⌃` and ⇧⌃` move focus between the image window and the shown panels (Window > Next Panel). "
            + "⌘` stays with macOS (next window), and ⌃F6 (focus the floating window) stays with macOS.",
    ]
}

extension PaintTool {
    /// The tool for a press of `key` from `current`: the next one sharing the
    /// key, or with `reverse` (Shift) the previous one.
    public static func tool(forKey key: Character, current: PaintTool, reverse: Bool) -> PaintTool? {
        guard reverse else { return tool(forKey: key, current: current) }
        let matches = allCases.filter { $0.key == key }
        guard let last = matches.last else { return nil }
        if let i = matches.firstIndex(of: current) { return matches[(i + matches.count - 1) % matches.count] }
        return last
    }
}

extension EditorWindowController {
    /// Canvas keys that are not menu items and not handled by the canvas
    /// itself (M59): Shift with a tool letter, the bracket keys for the brush
    /// width, Control-Tab for the next image. Returns true when it used the key.
    func canvasShortcut(_ event: NSEvent) -> Bool {
        let mods = event.modifierFlags.intersection([.command, .control, .option, .shift])
        switch (event.keyCode, mods) {
        case (33, []), (30, []), (33, [.command]), (30, [.command]):  // [ and ]
            let step: Double = mods.contains(.command) ? 5 : 1
            let w = workspace.tools.settings.brushWidth + (event.keyCode == 30 ? step : -step)
            workspace.tools.settings.brushWidth = min(max(w.rounded(), ToolSettings.widthRange.lowerBound),
                                                      ToolSettings.widthRange.upperBound)
            return true
        case (48, [.control]), (48, [.control, .shift]):  // Tab
            nextImage(forward: !mods.contains(.shift))
            return true
        default:
            break
        }
        if mods == [.shift], !event.isARepeat, let c = event.charactersIgnoringModifiers?.lowercased().first, c.isLetter,
           let t = PaintTool.tool(forKey: c, current: workspace.tools.tool, reverse: true) {
            workspace.tools.tool = t
            return true
        }
        return false
    }

    /// Activates the next (or previous) image in strip order.
    func nextImage(forward: Bool) {
        let all = workspace.editors
        guard all.count > 1, let i = all.firstIndex(where: { $0 === editor }) else { return }
        let next = all[(i + (forward ? 1 : all.count - 1)) % all.count]
        workspace.activate(next)
        if let w = next.windowController?.window, let c = next.windowController?.canvas { w.makeFirstResponder(c) }
    }
}

/// Window > Next Panel (⌃`) and Previous Panel (⇧⌃`): keyboard focus moves
/// from the image window through the shown panels in toolbar order (Tools,
/// History, Layers, Colors) and back. ⌘` is macOS's next-window key and ⌃F6
/// its focus-the-floating-window key, so neither is used.
@MainActor
public final class PanelFocusCycler: NSObject, NSMenuItemValidation {
    public static let shared = PanelFocusCycler()

    /// The ring: the image window, then the visible panels in toolbar order.
    public static func ring(document: NSWindow?) -> [NSWindow] {
        let panels = NSApp.windows.compactMap { $0 as? ShellPanel }.filter(\.isVisible)
        let ordered = PanelKind.allCases.compactMap { k in panels.first { $0.kind == k } }
        return (document.map { [$0] } ?? []) + ordered
    }

    /// The image window for a starting window: itself, a panel's router
    /// target, or the frontmost image window.
    static func documentWindow(from w: NSWindow?) -> NSWindow? {
        if let w = w as? EditorWindow, w.isVisible { return w }
        if let p = w as? ShellPanel, let doc = p.router?(), doc.isVisible { return doc }
        return NSApp.orderedWindows.first { $0 is EditorWindow && $0.isVisible }
    }

    /// Moves focus one step from `current` and returns the window that has it.
    @discardableResult
    public func cycle(forward: Bool, from current: NSWindow? = NSApp.keyWindow) -> NSWindow? {
        let ring = Self.ring(document: Self.documentWindow(from: current))
        guard !ring.isEmpty else { return nil }
        let i = current.flatMap { c in ring.firstIndex { $0 === c } }
        let next: NSWindow
        if let i { next = ring[(i + (forward ? 1 : ring.count - 1)) % ring.count] } else { next = ring[0] }
        focus(next)
        return next
    }

    func focus(_ w: NSWindow) {
        w.makeKeyAndOrderFront(nil)
        if let editor = (w as? EditorWindow)?.editorController {
            w.makeFirstResponder(editor.canvas)
        } else if let content = w.contentView {
            // The first control that takes focus, else the panel itself (VoiceOver
            // still reads the panel and its heading).
            w.makeFirstResponder(nil)
            w.selectKeyView(following: content)
        }
        NSAccessibility.post(element: w, notification: .focusedWindowChanged)
    }

    @objc public func focusNextPanel(_ sender: Any?) { cycle(forward: true) }
    @objc public func focusPreviousPanel(_ sender: Any?) { cycle(forward: false) }

    /// On while an image window is open and at least one panel shows.
    public func validateMenuItem(_ item: NSMenuItem) -> Bool {
        guard let doc = Self.documentWindow(from: NSApp.keyWindow), doc.isVisible else { return false }
        return Self.ring(document: doc).count > 1
    }
}
