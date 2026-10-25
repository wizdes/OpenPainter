// Helpers for the main-window (M15) tests: a workspace with its own engine and
// throwaway defaults, editors on fixture images, key events, and waiting.
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class ShellFixture {
    let engine: Engine
    let workspace: Workspace
    let defaults: UserDefaults
    let suite: String
    var controllers: [EditorWindowController] = []

    init() throws {
        _ = NSApplication.shared
        engine = try Engine()
        suite = "OpenPainterTests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)!
        workspace = Workspace(engine: engine, store: PanelStore(defaults: defaults), hidesPanelsOnDeactivate: false)
        workspace.startEvents()
    }

    /// Opens a fixture as an editor with a window (shown).
    @discardableResult
    func open(_ fixture: String = "plasma_alpha_512x384.png", name: String? = nil) throws -> EditorWindowController {
        let doc = try engine.openPNG(path: fixtures.appendingPathComponent(fixture).path)
        let info = try doc.info()
        let size = CGSize(width: (info["width"] as! NSNumber).doubleValue, height: (info["height"] as! NSNumber).doubleValue)
        let editor = Editor(document: doc, pixelSize: size, name: name ?? fixture)
        workspace.add(editor)
        let wc = EditorWindowController(editor: editor, workspace: workspace)
        wc.window?.isReleasedWhenClosed = false
        controllers.append(wc)
        wc.present()
        wc.window?.layoutIfNeeded()
        wc.updateClearRect()
        return wc
    }

    func tearDown() {
        workspace.stopEvents()
        for wc in controllers {
            wc.window?.attachedSheet.map { wc.window?.endSheet($0) }
            wc.window?.orderOut(nil)
            wc.closeWindow()
            try? wc.editor.document.close()
        }
        controllers.removeAll()
        workspace.panels.hideAll()
        for p in workspace.panels.panels.values { p.close() }
        engine.close()
        defaults.removePersistentDomain(forName: suite)
    }
}

/// Spins the main run loop until `condition` holds or `timeout` passes.
@MainActor
func waitUntil(_ timeout: TimeInterval = 10, _ condition: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if condition() { return true }
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    return condition()
}

/// A key-down event. `chars` is what the key types.
func keyEvent(_ chars: String, keyCode: UInt16, modifiers: NSEvent.ModifierFlags = [], window: NSWindow? = nil) -> NSEvent {
    NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: ProcessInfo.processInfo.systemUptime,
        windowNumber: window?.windowNumber ?? 0, context: nil, characters: chars, charactersIgnoringModifiers: chars,
        isARepeat: false, keyCode: keyCode)!
}

func functionKey(_ n: Int) -> String {
    String(Character(UnicodeScalar(UInt16(NSF1FunctionKey + n - 1))!))
}

/// Mac key codes for F5 to F8.
let functionKeyCodes: [Int: UInt16] = [5: 96, 6: 97, 7: 98, 8: 100]
/// Mac key codes for the digits 1 to 4.
let digitKeyCodes: [Int: UInt16] = [1: 18, 2: 19, 3: 20, 4: 21]
