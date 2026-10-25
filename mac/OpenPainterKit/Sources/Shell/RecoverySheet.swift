// M57: the one sheet at launch that lists the images a crash left unsaved,
// with Recover and Discard for each (docs/ARCHITECTURE.md, "Recovery
// records"). Recover opens the image with its unsaved changes; Discard
// deletes the recovery copy; Decide Later keeps the copies for the next launch.
// Sizes follow docs/design/SPEC.md: 12 pt rows, 11-12 pt secondary text in
// `muted`, 24 pt targets, `panel` body and `line` dividers.
import AppKit

@MainActor
public final class RecoverySheetController: NSWindowController {
    public static let width: CGFloat = 480
    public static let rowHeight: CGFloat = 56
    static let maxVisibleRows = 6

    public private(set) var records: [RecoveryRecord]
    let content = FilledView(fill: Theme.panel)
    public let titleLabel = Labels.label("Recover unsaved images?", size: 13, color: Theme.ink, weight: .semibold)
    public let messageLabel = Labels.label(
        "OpenPainter closed before these images were saved. Recover opens an image with its unsaved changes. "
            + "Discard deletes the recovery copy.", size: 12, color: Theme.muted)
    public private(set) var rows: [RecoveryRow] = []
    public private(set) var laterButton: NSButton!
    let scroll = NSScrollView()
    let list = FilledView(fill: Theme.panel)

    /// Opens the record's image off the main thread; throws when it cannot
    /// (the row shows why). The row shows a spinner meanwhile.
    public var onRecover: (@MainActor (RecoveryRecord) async throws -> Void)?
    /// Deletes the record; runs off the main thread.
    public var onDiscard: (@Sendable (RecoveryRecord) throws -> Void)?
    public var onClose: (@MainActor () -> Void)?

    public init(records: [RecoveryRecord]) {
        self.records = records
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: 200), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = "Recover Images"
        panel.isReleasedWhenClosed = false
        super.init(window: panel)
        panel.contentView = content
        build()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// A record's display name: the original file's name, or "Untitled".
    public static func name(_ r: RecoveryRecord) -> String {
        guard let url = r.originalURL, !url.lastPathComponent.isEmpty else { return "Untitled" }
        return url.isFileURL ? url.lastPathComponent : url.absoluteString
    }

    /// "Oct 25, 2:02 PM · 1200 × 760 px · 3 layers" (when it was recorded),
    /// or why it cannot be opened (the full problem is the tooltip).
    public static func detail(_ r: RecoveryRecord) -> String {
        if r.stale {
            // The full reason is the tooltip; the button says what Recover does.
            return "The file changed since this copy."
        }
        guard r.valid else { return "This copy is damaged and cannot be opened." }
        var parts: [String] = []
        if let t = r.time {
            let f = DateFormatter()
            f.setLocalizedDateFormatFromTemplate("MMMd jmm")
            parts.append(f.string(from: t))
        }
        if let w = r.width, let h = r.height { parts.append("\(w) × \(h) px") }
        if let n = r.layers { parts.append(n == 1 ? "1 layer" : "\(n) layers") }
        return parts.joined(separator: " · ")
    }

    // y grows down (FilledView is flipped).
    private func build() {
        let w = Self.width
        place(titleLabel, in: content, 16, 16, w - 32, 18)
        messageLabel.lineBreakMode = .byWordWrapping
        messageLabel.maximumNumberOfLines = 3
        messageLabel.cell?.wraps = true
        place(messageLabel, in: content, 16, 40, w - 32, 32)
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.documentView = list
        let later = NSButton(title: "Decide Later", target: self, action: #selector(later(_:)))
        later.font = Theme.font(12)
        later.keyEquivalent = "\u{1b}"
        laterButton = later
        layoutRows()
    }

    private func place(_ v: NSView, in parent: NSView, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) {
        v.translatesAutoresizingMaskIntoConstraints = true
        v.frame = NSRect(x: x, y: y, width: w, height: h)
        if v.superview !== parent { parent.addSubview(v) }
    }

    private func layoutRows() {
        for r in rows { r.removeFromSuperview() }
        rows = records.enumerated().map { i, record in
            let row = RecoveryRow(record: record, last: i == records.count - 1)
            row.recoverButton.target = self
            row.recoverButton.action = #selector(recover(_:))
            row.discardButton.target = self
            row.discardButton.action = #selector(discard(_:))
            return row
        }
        let w = Self.width
        list.frame = NSRect(x: 0, y: 0, width: w, height: CGFloat(rows.count) * Self.rowHeight)
        for (i, row) in rows.enumerated() {
            place(row, in: list, 0, CGFloat(i) * Self.rowHeight, w, Self.rowHeight)
        }
        let visible = CGFloat(min(rows.count, Self.maxVisibleRows)) * Self.rowHeight
        let top: CGFloat = 84
        place(scroll, in: content, 0, top, w, visible)
        content.dividers = []
        let y = top + visible + 16
        place(laterButton, in: content, w - 16 - 120, y, 120, 24)
        let height = y + 24 + 16
        window?.setContentSize(NSSize(width: w, height: height))
        content.frame = NSRect(x: 0, y: 0, width: w, height: height)
    }

    private func row(of sender: Any?) -> RecoveryRow? {
        rows.first { $0.recoverButton === sender as AnyObject || $0.discardButton === sender as AnyObject }
    }

    @objc public func recover(_ sender: Any?) {
        guard let row = row(of: sender), !row.busy else { return }
        row.setBusy(true)
        let action = onRecover
        let record = row.record
        Task { @MainActor [weak self, weak row] in
            do {
                try await action?(record)
                row?.setBusy(false)
                self?.remove(record)
            } catch {
                row?.setBusy(false)
                row?.show(problem: "This image could not be recovered: \(error.localizedDescription)")
            }
        }
    }

    /// Deletes the copy off the main thread (the folder's lock may make it
    /// wait); the row shows a spinner meanwhile (M57 review round 2).
    @objc public func discard(_ sender: Any?) {
        guard let row = row(of: sender), !row.busy else { return }
        row.setBusy(true)
        let action = onDiscard
        let record = row.record
        Task { @MainActor [weak self, weak row] in
            let result: Result<Void, any Error> = await Task.detached {
                Result { try action?(record) }
            }.value
            row?.setBusy(false)
            switch result {
            case .success: self?.remove(record)
            case .failure(let error):
                row?.show(problem: "The recovery copy could not be deleted: \(error.localizedDescription)")
            }
        }
    }

    @objc public func later(_ sender: Any?) { finish() }

    private func remove(_ record: RecoveryRecord) {
        records.removeAll { $0 == record }
        if records.isEmpty {
            finish()
        } else {
            layoutRows()
        }
    }

    private func finish() {
        if let sheet = window, let parent = sheet.sheetParent { parent.endSheet(sheet) }
        window?.orderOut(nil)
        let done = onClose
        onClose = nil
        done?()
    }

    /// Shows the sheet over `parent`, or as a centered window.
    public func present(on parent: NSWindow?) {
        if let parent {
            parent.beginSheet(window!) { _ in }
        } else {
            window?.center()
            window?.makeKeyAndOrderFront(nil)
        }
    }
}

/// One record: its name and details on the left, Recover and Discard on the right.
@MainActor
public final class RecoveryRow: FilledView {
    public let record: RecoveryRecord
    public let nameLabel: NSTextField
    public let detailLabel: NSTextField
    public let recoverButton = NSButton(title: "Recover", target: nil, action: nil)
    /// Shown while Recover opens the image.
    public let spinner = NSProgressIndicator()
    public private(set) var busy = false
    public let discardButton = NSButton(title: "Discard", target: nil, action: nil)

    init(record: RecoveryRecord, last: Bool) {
        self.record = record
        nameLabel = Labels.label(RecoverySheetController.name(record), size: 12, color: Theme.ink, weight: .semibold)
        detailLabel = Labels.label(RecoverySheetController.detail(record), size: 11, color: Theme.muted)
        super.init(fill: Theme.panel, dividers: last ? [.top, .bottom] : [.top])
        let w = RecoverySheetController.width
        nameLabel.toolTip = record.originalURL?.path
        detailLabel.toolTip = record.problem.map { "Recorded copy problem: \($0)" }
        for b in [recoverButton, discardButton] {
            b.font = Theme.font(12)
            b.bezelStyle = .rounded
        }
        if record.stale { recoverButton.title = "Recover as Untitled" }
        recoverButton.isEnabled = record.valid || record.stale
        recoverButton.setAccessibilityLabel("Recover \(nameLabel.stringValue)")
        discardButton.setAccessibilityLabel("Discard the recovery copy of \(nameLabel.stringValue)")
        add(nameLabel, 16, 10, w - 16 - 200 - 8, 16)
        add(detailLabel, 16, 30, w - 16 - 200 - 8, 16)
        if record.stale {
            nameLabel.frame.size.width = w - 16 - 260 - 8
            detailLabel.frame.size.width = w - 16 - 260 - 8
        }
        let recoverWidth: CGFloat = record.stale ? 140 : 88
        add(recoverButton, w - 16 - 88 - 8 - recoverWidth, 16, recoverWidth, 24)
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        spinner.isHidden = true
        spinner.setAccessibilityLabel("Recovering \(nameLabel.stringValue)")
        add(spinner, w - 16 - 88 - 8 - recoverWidth - 8 - 16, 20, 16, 16)
        add(discardButton, w - 16 - 88, 16, 88, 24)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    private func add(_ v: NSView, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) {
        v.translatesAutoresizingMaskIntoConstraints = true
        v.frame = NSRect(x: x, y: y, width: w, height: h)
        addSubview(v)
    }

    func setBusy(_ on: Bool) {
        busy = on
        spinner.isHidden = !on
        if on { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
        recoverButton.isEnabled = !on && (record.valid || record.stale)
        discardButton.isEnabled = !on
    }

    func show(problem: String) {
        detailLabel.stringValue = problem
        detailLabel.toolTip = problem
    }
}
