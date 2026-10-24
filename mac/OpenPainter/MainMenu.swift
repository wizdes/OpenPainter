// The temporary menu bar for M12. M15 builds the full menu from the command catalog.
//
// Shortcut mapping from paint.net (Windows) to the Mac:
//   Zoom In        Ctrl++            -> Cmd+=
//   Zoom Out       Ctrl+-            -> Cmd+-
//   Zoom to Window Ctrl+B (3.36) / Ctrl+0 (brief) -> Cmd+0
//   Actual Size    Ctrl+Shift+A (3.36) / Ctrl+0 is taken -> Cmd+Option+0
//   Pixel Grid     (no shortcut)     -> Cmd+'
//   Zoom tool Z, Pan tool H, hold Space for a temporary pan: same keys.
import AppKit

enum MainMenu {
    static func build() -> NSMenu {
        let main = NSMenu(title: "Main")
        let appName = "OpenPainter"

        let appMenu = NSMenu(title: appName)
        appMenu.addItem(withTitle: "About \(appName)", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide \(appName)", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(withTitle: "Quit \(appName)", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        add(appMenu, to: main)

        let file = NSMenu(title: "File")
        file.addItem(withTitle: "Open…", action: #selector(NSDocumentController.openDocument(_:)), keyEquivalent: "o")
        // NSDocumentController fills a submenu that holds a "Clear Menu" item
        // with clearRecentDocuments: and keeps it up to date.
        let recent = NSMenu(title: "Open Recent")
        recent.addItem(withTitle: "Clear Menu", action: #selector(NSDocumentController.clearRecentDocuments(_:)), keyEquivalent: "")
        let recentItem = file.addItem(withTitle: "Open Recent", action: nil, keyEquivalent: "")
        recentItem.submenu = recent
        file.addItem(.separator())
        file.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        add(file, to: main)

        let view = NSMenu(title: "View")
        view.addItem(withTitle: "Zoom In", action: #selector(MetalCanvasViewActions.zoomIn(_:)), keyEquivalent: "=")
        view.addItem(withTitle: "Zoom Out", action: #selector(MetalCanvasViewActions.zoomOut(_:)), keyEquivalent: "-")
        view.addItem(withTitle: "Zoom to Window", action: #selector(MetalCanvasViewActions.zoomToWindow(_:)), keyEquivalent: "0")
        let actual = view.addItem(withTitle: "Actual Size", action: #selector(MetalCanvasViewActions.actualSize(_:)), keyEquivalent: "0")
        actual.keyEquivalentModifierMask = [.command, .option]
        view.addItem(.separator())
        view.addItem(withTitle: "Pixel Grid", action: #selector(MetalCanvasViewActions.togglePixelGrid(_:)), keyEquivalent: "'")
        add(view, to: main)

        let tools = NSMenu(title: "Tools")
        tools.addItem(withTitle: "Pan", action: #selector(MetalCanvasViewActions.selectPanTool(_:)), keyEquivalent: "")
        tools.addItem(withTitle: "Zoom", action: #selector(MetalCanvasViewActions.selectZoomTool(_:)), keyEquivalent: "")
        add(tools, to: main)

        let window = NSMenu(title: "Window")
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        add(window, to: main)
        NSApp.windowsMenu = window
        return main
    }

    private static func add(_ menu: NSMenu, to main: NSMenu) {
        let item = NSMenuItem(title: menu.title, action: nil, keyEquivalent: "")
        item.submenu = menu
        main.addItem(item)
    }
}

/// Selector names for the canvas commands (implemented by MetalCanvasView, found
/// through the responder chain).
@objc protocol MetalCanvasViewActions {
    func zoomIn(_ sender: Any?)
    func zoomOut(_ sender: Any?)
    func zoomToWindow(_ sender: Any?)
    func actualSize(_ sender: Any?)
    func togglePixelGrid(_ sender: Any?)
    func selectPanTool(_ sender: Any?)
    func selectZoomTool(_ sender: Any?)
}
