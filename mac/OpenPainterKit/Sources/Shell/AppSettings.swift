// The app's settings (M56): appearance, units, rulers and the pixel grid. They
// are kept in UserDefaults under `OPSettings.*` and apply to every open window
// at once. The View menu, the toolbar, the status bar and the Settings window
// all change the same values.
//
// paint.net keeps the units, the rulers and the pixel grid as app-wide
// choices (https://www.getpaint.net/doc/latest/ViewMenu.html) and has a
// light or dark choice in its Settings dialog
// (https://www.getpaint.net/doc/latest/SettingsDialog.html, User Interface).
import AppKit

/// Settings > Appearance.
public enum AppearanceChoice: String, CaseIterable, Sendable {
    case system, light, dark

    public var title: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    /// The app appearance to set (nil follows the system).
    public var appearance: NSAppearance? {
        switch self {
        case .system: nil
        case .light: NSAppearance(named: .aqua)
        case .dark: NSAppearance(named: .darkAqua)
        }
    }
}

@MainActor
public final class AppSettings {
    public let defaults: UserDefaults
    let prefix: String
    private struct Observer {
        weak var owner: AnyObject?
        let body: () -> Void
    }
    private var observers: [Observer] = []

    public init(defaults: UserDefaults = .standard, prefix: String = "OPSettings") {
        self.defaults = defaults
        self.prefix = prefix
    }

    private func key(_ name: String) -> String { "\(prefix).\(name)" }

    public var appearance: AppearanceChoice {
        get { defaults.string(forKey: key("appearance")).flatMap(AppearanceChoice.init(rawValue:)) ?? .system }
        set { set(newValue.rawValue, "appearance", changed: newValue != appearance) }
    }

    /// The units for rulers and the status bar. Pixels by default, as in paint.net.
    public var units: MeasurementUnit {
        get { defaults.string(forKey: key("units")).flatMap(MeasurementUnit.init(rawValue:)) ?? .pixels }
        set { set(newValue.rawValue, "units", changed: newValue != units) }
    }

    /// View > Rulers. Off by default.
    public var rulers: Bool {
        get { defaults.bool(forKey: key("rulers")) }
        set { set(newValue, "rulers", changed: newValue != rulers) }
    }

    /// View > Pixel Grid. Off by default; drawn only at 400% and above.
    public var pixelGrid: Bool {
        get { defaults.bool(forKey: key("pixelGrid")) }
        set { set(newValue, "pixelGrid", changed: newValue != pixelGrid) }
    }

    private func set(_ value: Any, _ name: String, changed: Bool) {
        guard changed else { return }
        defaults.set(value, forKey: key(name))
        notify()
    }

    /// Calls `body` after every change while `owner` lives.
    public func observe(_ owner: AnyObject, _ body: @escaping () -> Void) {
        observers.append(Observer(owner: owner, body: body))
    }

    func notify() {
        observers.removeAll { $0.owner == nil }
        for o in observers { o.body() }
    }

    /// Sets the app's appearance from the setting. Panels, dialogs and the
    /// canvas surround follow through the dynamic theme tokens.
    public func applyAppearance(to app: NSApplication = .shared) {
        let want = appearance.appearance
        if app.appearance?.name != want?.name { app.appearance = want }
    }
}
