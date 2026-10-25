// The menu bar: the full paint.net 5.1 skeleton from OpenPainterKit's
// MenuSkeleton (its header lists the Windows-to-Mac shortcut changes), with
// Adjustments and Effects built from the engine's effect registry.
import AppKit
import OpenPainterKit

enum MainMenu {
    @MainActor
    static func build(_ workspace: Workspace) -> NSMenu {
        let main = MenuSkeleton.build(effects: workspace.effects, panelTarget: workspace.menuTarget)
        if let window = main.item(withTitle: "Window")?.submenu { NSApp.windowsMenu = window }
        if let help = main.item(withTitle: "Help")?.submenu { NSApp.helpMenu = help }
        return main
    }
}
