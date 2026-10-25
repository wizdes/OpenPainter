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
        removeDefaultsSuite(suite)
    }
}

/// Deletes a test's preferences domain and its plist file.
/// `removePersistentDomain` alone leaves an empty
/// ~/Library/Preferences/<suite>.plist behind (M58 found thousands), and
/// objects that still hold the domain can write that empty file again after
/// the test ends, so the suite is also swept once more when the test bundle
/// finishes (`PreferencesJanitor`).
@MainActor
func removeDefaultsSuite(_ suite: String) {
    PreferencesJanitor.shared.remove(suite)
}

/// Deletes test preference domains now and again when the test bundle ends.
@MainActor
final class PreferencesJanitor: NSObject, XCTestObservation {
    static let shared = PreferencesJanitor()
    private var suites: Set<String> = []
    private var observing = false

    static func file(_ suite: String) -> URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Preferences/\(suite).plist")
    }

    func remove(_ suite: String) {
        if !observing {
            observing = true
            XCTestObservationCenter.shared.addTestObserver(self)
        }
        suites.insert(suite)
        Self.wipe(suite)
    }

    static func wipe(_ suite: String) {
        UserDefaults.standard.removePersistentDomain(forName: suite)
        CFPreferencesAppSynchronize(suite as CFString)
        try? FileManager.default.removeItem(at: file(suite))
    }

    nonisolated func testBundleDidFinish(_ testBundle: Bundle) {
        MainActor.assumeIsolated {
            // Late writers (released defaults objects) have had their turn by now.
            RunLoop.main.run(until: Date().addingTimeInterval(2))
            for suite in suites { Self.wipe(suite) }
            let left = suites.filter { FileManager.default.fileExists(atPath: Self.file($0).path) }
            if !left.isEmpty {
                FileHandle.standardError.write(Data("PreferencesJanitor: \(left.count) test preference files came back: \(left.sorted())\n".utf8))
            }
        }
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

extension EditorWindowController {
    /// A More menu item (the old toolbar's other commands), validated as the
    /// menu validates it when it opens.
    func moreItem(_ title: String) -> NSMenuItem? {
        let menu = moreMenu()
        menu.update()
        return menu.items.first { $0.title == title }
    }

    /// Chooses a More menu item, as a click on it does.
    func clickMoreItem(_ title: String) {
        let menu = moreMenu()
        menu.update()
        guard let item = menu.items.first(where: { $0.title == title }) else { return XCTFail("no More item \(title)") }
        menu.performActionForItem(at: menu.index(of: item))
    }
}
