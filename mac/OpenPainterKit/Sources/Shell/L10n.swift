// Strings ready for translation (M59).
//
// Every user-facing string goes through `L10n`. The English text is the key
// (the String Catalog convention), and `Localizable.xcstrings` next to this file
// holds every key with a comment that says where the string shows. Xcode
// compiles the catalog into the OpenPainterKit bundle.
//
// - `L10n.tr(_:_:)` translates one string at the place it is set.
// - `L10n.localize(_:context:)` walks a finished view tree and translates what
//   the views show: label text, button titles, popup items, segment labels,
//   placeholders, tooltips and accessibility labels. Each window, panel and
//   dialog calls it once after it is built, so strings set at build time need
//   no change at their call site. Editable text (a field's value) is left alone.
// - Engine labels (effect names, history step names) are keyed by command id:
//   `engine.<command id>`, with the engine's English as the fallback. The
//   catalog's English table lists every effect id from `effects.list`.
// - Pseudo-localization (`L10n.pseudo`) makes every translated string about
//   twice as long, with accented letters, to find clipped text before a real
//   translation exists. `KeyboardAccessibilityTests` renders the main window,
//   the panels and the dialogs with it.
import AppKit

public enum L10n {
    /// Doubles every translated string (accented letters, padded to twice the
    /// length, in brackets).
    public static var pseudo = false
    /// While true, every key looked up is recorded with its comment.
    public static var recording = false
    /// Keys looked up while `recording`, with the first comment given.
    public private(set) static var used: [String: String] = [:]

    public static func resetUsed() { used = [:] }

    /// The framework bundle that holds the compiled catalog.
    static let bundle = Bundle(for: BundleMarker.self)
    private final class BundleMarker {}

    /// Translates `english`. `comment` says where it shows (for translators).
    public static func tr(_ english: String, _ comment: String) -> String {
        guard isTranslatable(english) else { return english }
        if recording, used[english] == nil { used[english] = comment }
        let s = bundle.localizedString(forKey: english, value: english, table: nil)
        return pseudo ? pseudoString(s) : s
    }

    /// An engine label keyed by command id (effect names, history step names).
    public static func engine(_ id: String, english: String) -> String {
        guard !id.isEmpty else { return tr(english, "Engine label") }
        let key = "engine." + id
        if recording, used[key] == nil { used[key] = "Engine label for the command \(id). English: \(english)" }
        // In English the engine's own label shows (it can carry details the
        // table does not); other languages take the table's text for the id.
        let inEnglish = bundle.preferredLocalizations.first?.hasPrefix("en") ?? true
        let s = inEnglish ? english : bundle.localizedString(forKey: key, value: english, table: nil)
        return pseudo ? pseudoString(s) : s
    }

    /// True for text a person reads: it has a letter and is not a hex color,
    /// a number with a unit, or a single shortcut glyph.
    static func isTranslatable(_ s: String) -> Bool {
        let t = s.trimmingCharacters(in: .whitespaces)
        guard t.count > 1, t.contains(where: \.isLetter) else { return false }
        if t.hasPrefix("#"), DocColor(hex: t) != nil { return false }
        if t.first?.isNumber == true, t.filter(\.isLetter).count <= 3 { return false }  // "100%", "96 px", "8 bpp"
        return true
    }

    // MARK: Pseudo-localization

    private static let accents: [Character: Character] = [
        "a": "å", "b": "ƀ", "c": "ç", "d": "ð", "e": "é", "f": "ƒ", "g": "ĝ", "h": "ĥ", "i": "î", "j": "ĵ",
        "k": "ķ", "l": "ļ", "m": "ɱ", "n": "ñ", "o": "ö", "p": "þ", "r": "ŕ", "s": "š", "t": "ţ", "u": "û",
        "v": "ṽ", "w": "ŵ", "y": "ý", "z": "ž", "A": "Å", "B": "Ɓ", "C": "Ç", "D": "Ð", "E": "É", "F": "Ƒ",
        "G": "Ĝ", "H": "Ĥ", "I": "Î", "J": "Ĵ", "K": "Ķ", "L": "Ļ", "M": "Ṁ", "N": "Ñ", "O": "Ö", "P": "Þ",
        "R": "Ŕ", "S": "Š", "T": "Ţ", "U": "Û", "V": "Ṽ", "W": "Ŵ", "Y": "Ý", "Z": "Ž",
    ]

    /// "Open" -> "[Öþéñ·Öþéñ]": accented, then padded with its own start to
    /// twice the length (plus three marks). Format specifiers and shortcut
    /// glyphs stay.
    public static func pseudoString(_ s: String) -> String {
        if s.hasPrefix("[") && s.hasSuffix("]") { return s }  // already pseudo
        // Accent every letter except the ones in format specifiers (%d, %@, %1$@).
        var accented = ""
        var inSpec = false
        for c in s {
            if inSpec {
                accented.append(c)
                if c.isLetter || c == "@" { inSpec = false }
            } else if c == "%" {
                accented.append(c)
                inSpec = true
            } else {
                accented.append(accents[c] ?? c)
            }
        }
        let pad = max(s.count, 1)
        var padding = String(s.replacingOccurrences(of: "%", with: "").map { accents[$0] ?? $0 }.prefix(pad))
            .replacingOccurrences(of: "@", with: "·")
        while padding.count < pad { padding += "·" }
        return "[" + accented + "·" + padding + "]"
    }

    // MARK: View trees

    /// Translates what the views under `root` show. `context` names the window
    /// or dialog, for the recorded comments.
    @MainActor public static func localize(_ root: NSView, context: String) {
        // Rows and swatches show user data (layer names, file names, colors)
        // and translate their own fixed words.
        if root is PanelRow || root is PaletteCell || root is ImageStripItem { return }
        localizeView(root, context: context)
        for v in root.subviews { localize(v, context: context) }
    }

    /// Translates a window's title, its content and its toolbar items.
    @MainActor public static func localize(_ window: NSWindow, context: String) {
        if !window.title.isEmpty { window.title = tr(window.title, "\(context): window title") }
        if let content = window.contentView { localize(content, context: context) }
        for item in window.toolbar?.items ?? [] {
            if !item.label.isEmpty { item.label = tr(item.label, "\(context): toolbar group name") }
            if let v = item.view { localize(v, context: context) }
        }
    }

    @MainActor private static func localizeView(_ v: NSView, context: String) {
        if let tip = v.toolTip, !tip.isEmpty { v.toolTip = tr(tip, "\(context): tooltip") }
        if let label = v.accessibilityLabel(), !label.isEmpty {
            v.setVoiceOverLabel(tr(label, "\(context): VoiceOver label"))
        }
        switch v {
        case let p as NSPopUpButton:
            for item in p.itemArray where !item.title.isEmpty {
                item.title = tr(item.title, "\(context): menu choice in a popup")
            }
            p.synchronizeTitleAndSelectedItem()
        case let s as NSSegmentedControl:
            for i in 0..<s.segmentCount {
                if let l = s.label(forSegment: i), !l.isEmpty {
                    s.setLabel(tr(l, "\(context): segment"), forSegment: i)
                }
                if let t = s.toolTip(forSegment: i), !t.isEmpty {
                    s.setToolTip(tr(t, "\(context): segment tooltip"), forSegment: i)
                }
            }
        case let b as NSButton:
            // Image-only buttons keep AppKit's unused default title ("Button").
            if !b.title.isEmpty, b.imagePosition != .imageOnly {
                b.title = tr(b.title, "\(context): button or checkbox title")
            }
        case let c as NSComboBox:
            if let ph = c.placeholderString { c.placeholderString = tr(ph, "\(context): placeholder") }
        case let f as NSTextField:
            if let ph = f.placeholderString { f.placeholderString = tr(ph, "\(context): placeholder") }
            if !f.isEditable, !f.stringValue.isEmpty { f.stringValue = tr(f.stringValue, "\(context): label") }
        default:
            break
        }
    }

    // MARK: Fitting

    /// True when layouts may grow for longer text: a pseudo run, or a
    /// language other than English. In English every size stays as
    /// docs/design/SPEC.md gives it.
    public static var adjustsLayout: Bool {
        pseudo || !(bundle.preferredLocalizations.first?.hasPrefix("en") ?? true)
    }

    /// How much wider than its view a text control's text is (1 when it fits).
    @MainActor static func overflow(_ v: NSView) -> CGFloat {
        guard !v.isHiddenOrHasHiddenAncestor, v.bounds.width > 0 else { return 1 }
        return max(1, neededWidth(v) / v.bounds.width)
    }

    /// The width a text control's text needs (0 for other views).
    @MainActor static func neededWidth(_ v: NSView) -> CGFloat {
        var need: CGFloat = 0
        switch v {
        case let b as NSButton where b.imagePosition != .imageOnly && !b.title.isEmpty:
            need = b.cell?.cellSize.width ?? 0
        case let p as NSPopUpButton:
            // The widest choice, not only the shown one.
            let shown = p.indexOfSelectedItem
            for i in 0..<p.numberOfItems where !(p.item(at: i)?.isHidden ?? true) {
                p.selectItem(at: i)
                need = max(need, p.cell?.cellSize.width ?? 0)
            }
            p.selectItem(at: shown)
        case let f as NSTextField where !f.isEditable && !f.stringValue.isEmpty:
            if f.cell?.wraps == true { return 0 }
            need = f.cell?.cellSize.width ?? 0
        default:
            return 0
        }
        return need
    }

    /// Grows the fixed width constraint of each button or popup whose text
    /// no longer fits (constraint-based rows: the options row, Settings).
    @MainActor public static func growWidthConstraints(in root: NSView) {
        guard adjustsLayout else { return }
        for v in [root] + root.descendantViews where !v.translatesAutoresizingMaskIntoConstraints {
            guard !(v is NSTextField),
                  let c = v.constraints.first(where: { $0.firstAttribute == .width && $0.relation == .equal
                      && $0.secondItem == nil && $0.firstItem === v }) else { continue }
            let need = neededWidth(v).rounded(.up)
            if need > c.constant + 1 { c.constant = need }
        }
    }

    /// Widens a frame-based dialog so its translated text fits: every view
    /// laid out by frame (not by constraints) is stretched along x by the
    /// largest overflow, up to 3 times, so the layout keeps its columns.
    /// Constraint-based views size themselves and are left alone.
    @MainActor public static func widenToFit(_ window: NSWindow) {
        guard adjustsLayout else { return }
        guard let content = window.contentView else { return }
        let all = content.descendantViews
        let factor = min(all.map(overflow).max() ?? 1, 3)
        guard factor > 1.01 else { return }
        func stretch(_ v: NSView) {
            for s in v.subviews where s.translatesAutoresizingMaskIntoConstraints {
                if !(v is NSControl) && !(v is NSClipView) && !(v is NSScrollView) {
                    s.frame = NSRect(x: (s.frame.minX * factor).rounded(), y: s.frame.minY,
                                     width: (s.frame.width * factor).rounded(), height: s.frame.height)
                    if !(s is NSControl) { stretch(s) }
                }
            }
        }
        let auto = content.autoresizesSubviews
        content.autoresizesSubviews = false
        var size = content.frame.size
        size.width = (size.width * factor).rounded()
        window.setContentSize(size)
        stretch(content)
        content.autoresizesSubviews = auto
    }

    /// Translates a menu and its submenus (titles only; key equivalents stay).
    @MainActor public static func localize(_ menu: NSMenu, context: String) {
        for item in menu.items where !item.isSeparatorItem {
            if !item.title.isEmpty {
                if let id = item.representedObject as? String, id.hasPrefix("effect.") {
                    item.title = engine(id, english: item.title)
                } else {
                    item.title = tr(item.title, "\(context): menu item")
                }
            }
            if let tip = item.toolTip { item.toolTip = tr(tip, "\(context): menu item tooltip") }
            if let sub = item.submenu {
                sub.title = item.title
                localize(sub, context: context)
            }
        }
    }
}

extension NSView {
    /// Sets the accessibility label on the view and, for a control, on its
    /// cell too: the accessibility tree AppKit hands VoiceOver lists a
    /// control's cell, and the cell does not read the control's label.
    @MainActor public func setVoiceOverLabel(_ label: String) {
        setAccessibilityLabel(label)
        (self as? NSControl)?.cell?.setAccessibilityLabel(label)
    }

    /// Every view under this one, depth first.
    public var descendantViews: [NSView] { subviews.flatMap { [$0] + $0.descendantViews } }
}
