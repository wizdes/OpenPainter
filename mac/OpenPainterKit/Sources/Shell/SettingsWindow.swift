// OpenPainter > Settings… (Cmd+,), M56. One page with the paint.net settings
// that apply to features that exist now
// (https://www.getpaint.net/doc/latest/SettingsDialog.html): the light or dark
// appearance (paint.net's User Interface tab), and the View menu's units,
// rulers and pixel grid, which paint.net remembers between runs. Every change
// is saved in UserDefaults and applies to all open windows at once.
import AppKit

@MainActor
public final class SettingsWindowController: NSWindowController, NSWindowDelegate {
    public let settings: AppSettings
    public let appearancePopup = Labels.popup(AppearanceChoice.allCases.map(\.title), width: 160)
    public let unitsPopup = Labels.popup(MeasurementUnit.allCases.map(\.title), width: 160)
    public let rulersCheck = NSButton(checkboxWithTitle: "Show rulers", target: nil, action: nil)
    public let gridCheck = NSButton(checkboxWithTitle: "Show the pixel grid", target: nil, action: nil)

    /// The window for the app's settings; made on first use.
    public static var shared: SettingsWindowController?

    public static func show(settings: AppSettings) {
        if shared == nil || shared?.settings !== settings { shared = SettingsWindowController(settings: settings) }
        shared?.showWindow(nil)
        shared?.window?.makeKeyAndOrderFront(nil)
    }

    public init(settings: AppSettings) {
        self.settings = settings
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 228), styleMask: [.titled, .closable],
            backing: .buffered, defer: false)
        window.title = "Settings"
        window.isRestorable = false
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        super.init(window: window)
        window.delegate = self
        let root = content()
        window.contentView = root
        root.layoutSubtreeIfNeeded()
        let fit = root.subviews.first?.fittingSize ?? .zero
        window.setContentSize(NSSize(width: max(420, fit.width + 48), height: fit.height + 32))
        window.center()
        settings.observe(self) { [weak self] in self?.reload() }
        reload()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func content() -> NSView {
        let root = FilledView(fill: Theme.panel)
        func label(_ text: String) -> NSTextField { Labels.label(text, size: 13, color: Theme.ink) }
        func note(_ text: String) -> NSTextField { Labels.label(text, size: 11, color: Theme.muted) }
        for (c, name) in [(appearancePopup, "Appearance"), (unitsPopup, "Units")] {
            c.target = self
            c.action = #selector(changed(_:))
            c.setAccessibilityLabel(name)
        }
        for c in [rulersCheck, gridCheck] {
            c.target = self
            c.action = #selector(changed(_:))
            c.font = Theme.font(13)
            c.contentTintColor = Theme.accent
            (c.cell as? NSButtonCell)?.attributedTitle = NSAttributedString(
                string: c.title, attributes: [.font: Theme.font(13), .foregroundColor: Theme.ink])
        }
        let title = Labels.label("View", size: 13, color: Theme.ink, weight: .semibold)
        let look = Labels.label("User Interface", size: 13, color: Theme.ink, weight: .semibold)
        let grid = NSGridView(views: [
            [look, NSGridCell.emptyContentView],
            [label("Appearance:"), appearancePopup],
            [NSGridCell.emptyContentView, note("System follows the Mac's light or dark setting.")],
            [title, NSGridCell.emptyContentView],
            [label("Units:"), unitsPopup],
            [NSGridCell.emptyContentView, rulersCheck],
            [NSGridCell.emptyContentView, gridCheck],
            [NSGridCell.emptyContentView, note("The grid shows at 400% zoom and above.")],
        ])
        grid.translatesAutoresizingMaskIntoConstraints = false
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.rowAlignment = .firstBaseline
        grid.row(at: 3).topPadding = 8
        for r in [0, 3] { grid.cell(atColumnIndex: 0, rowIndex: r).xPlacement = .leading }
        grid.mergeCells(inHorizontalRange: NSRange(location: 0, length: 2), verticalRange: NSRange(location: 0, length: 1))
        grid.mergeCells(inHorizontalRange: NSRange(location: 0, length: 2), verticalRange: NSRange(location: 3, length: 1))
        root.addSubview(grid)
        NSLayoutConstraint.activate([
            grid.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 24),
            grid.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -24),
            grid.topAnchor.constraint(equalTo: root.topAnchor, constant: 16),
            grid.bottomAnchor.constraint(lessThanOrEqualTo: root.bottomAnchor, constant: -16),
        ])
        return root
    }

    /// Shows the stored values.
    func reload() {
        appearancePopup.selectItem(at: AppearanceChoice.allCases.firstIndex(of: settings.appearance) ?? 0)
        unitsPopup.selectItem(at: MeasurementUnit.allCases.firstIndex(of: settings.units) ?? 0)
        rulersCheck.state = settings.rulers ? .on : .off
        gridCheck.state = settings.pixelGrid ? .on : .off
    }

    @objc func changed(_ sender: Any?) {
        switch sender as? NSControl {
        case appearancePopup: settings.appearance = AppearanceChoice.allCases[max(appearancePopup.indexOfSelectedItem, 0)]
        case unitsPopup: settings.units = MeasurementUnit.allCases[max(unitsPopup.indexOfSelectedItem, 0)]
        case rulersCheck: settings.rulers = rulersCheck.state == .on
        case gridCheck: settings.pixelGrid = gridCheck.state == .on
        default: break
        }
    }
}

/// Target of OpenPainter > Settings… .
@MainActor @objc public protocol SettingsMenuActions {
    func showSettings(_ sender: Any?)
}

@MainActor
public final class SettingsMenuTarget: NSObject, SettingsMenuActions {
    public static let shared = SettingsMenuTarget()

    @objc public func showSettings(_ sender: Any?) {
        guard let settings = Workspace.shared?.settings else {
            FileHandle.standardError.write(Data("OpenPainter: Settings opened before the workspace exists\n".utf8))
            return
        }
        SettingsWindowController.show(settings: settings)
    }
}
