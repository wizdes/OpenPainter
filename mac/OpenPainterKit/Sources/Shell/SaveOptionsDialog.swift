// M50: the save options sheet (paint.net's Save Configuration dialog,
// https://www.getpaint.net/doc/latest/FileMenu.html, Save). It shows the
// chosen format's options, the metadata choice, the file size at those
// options and, for lossy formats, the image as it will look once saved. The
// size comes from a background encode (SaveSizePreview) that restarts on each
// change; only the latest result shows.
//
// Layout follows the other Classic v2 dialogs (Resize, Canvas Size): panel
// fill, 16 pt insets, 12 pt labels 8 pt from their control, 28 pt controls and
// buttons, `muted` section labels, `line` frame round the preview.
import AppKit

@MainActor
public final class SaveOptionsController: NSWindowController, NSTextFieldDelegate {
    public let type: String
    public let kind: SaveOptionKind
    public private(set) var options: SaveOptions
    public let hasMetadata: Bool
    let content = FilledView(fill: Theme.panel)

    public let qualitySlider = NSSlider(value: 95, minValue: 1, maxValue: 100, target: nil, action: nil)
    public let qualityField = Labels.field("95", width: 52)
    public let subsamplingPopup = Labels.popup(ChromaSubsampling.allCases.map(\.label), width: 220)
    public let levelSlider = NSSlider(value: 6, minValue: 0, maxValue: 9, target: nil, action: nil)
    public let levelValue = Labels.label("6", color: Theme.ink)
    public let tiffPopup = Labels.popup(TIFFCompression.allCases.map(\.label), width: 160)
    public let ditherBox = NSButton(checkboxWithTitle: "Dithering", target: nil, action: nil)
    public let metadataPopup = Labels.popup(MetadataChoice.allCases.map(\.label), width: 260)
    /// "File size: 123 KB", or "Measuring…" while the encode runs.
    public let sizeLabel = Labels.label("Measuring…", size: 12, color: Theme.ink, weight: .semibold)
    public let noteLabel = Labels.label("", size: 11)
    public let previewView = NSImageView()
    let previewFrame = FilledView(fill: Theme.stage)
    public private(set) var okButton: NSButton!
    public private(set) var cancelButton: NSButton!
    public let preview: SaveSizePreview
    /// The last size shown, in bytes.
    public private(set) var shownBytes: Int?
    /// Called once when the sheet closes: the options for OK, nil for Cancel.
    public var onClose: ((SaveOptions?) -> Void)?

    public static let width: CGFloat = 440

    public init(type: String, options: SaveOptions, pixels: SavePixels, metadata: ImageMetadata?) {
        let kind = SaveOptionKind(type: type) ?? .png
        self.type = type
        self.kind = kind
        self.options = options.clamped(for: type)
        self.hasMetadata = !(metadata?.isEmpty ?? true)
        preview = SaveSizePreview(decodeImage: kind.isLossy) { o in
            try SaveEncoder.encode(pixels, type: type, options: o, metadata: metadata)
        }
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: 200), styleMask: [.titled],
                            backing: .buffered, defer: true)
        panel.title = "Save Options: \(ImageFormats.shortName(type))"
        super.init(window: panel)
        panel.contentView = content
        let height = build()
        panel.setContentSize(NSSize(width: Self.width, height: height))
        preview.onResult = { [weak self] r in self?.show(r) }
        show()
        preview.request(self.options)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    // y grows down (FilledView is flipped).
    func place(_ v: NSView, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat = 24) {
        v.translatesAutoresizingMaskIntoConstraints = true
        v.frame = NSRect(x: x, y: y, width: w, height: h)
        content.addSubview(v)
    }

    func label(_ text: String, _ y: CGFloat, color: NSColor = Theme.ink) {
        let l = Labels.label(text, color: color)
        l.alignment = .right
        place(l, 16, y + 4, 128, 16)
    }

    func section(_ text: String, _ y: CGFloat) {
        let l = Labels.label(text, color: Theme.muted)
        place(l, 16, y, 300, 16)
    }

    /// Lays out the controls for this format; returns the content height.
    private func build() -> CGFloat {
        let x: CGFloat = 152
        var y: CGFloat = 16
        section(ImageFormats.shortName(type) + " options", y)
        y += 24
        switch kind {
        case .jpeg, .heic, .avif, .webp:
            label("Quality:", y)
            qualitySlider.maxValue = Double(kind.maxQuality)
            place(qualitySlider, x, y, 200, 24)
            place(qualityField, x + 208, y, 52, 24)
            qualitySlider.setAccessibilityLabel("Quality")
            qualityField.setAccessibilityLabel("Quality value")
            qualitySlider.target = self
            qualitySlider.action = #selector(controlChanged(_:))
            qualityField.delegate = self
            qualityField.target = self
            qualityField.action = #selector(fieldTyped(_:))
            qualityField.alignment = .right
            y += 32
            if kind == .jpeg {
                label("Chroma subsampling:", y)
                place(subsamplingPopup, x, y - 2, 220, 28)
                subsamplingPopup.setAccessibilityLabel("Chroma subsampling")
                subsamplingPopup.target = self
                subsamplingPopup.action = #selector(controlChanged(_:))
                y += 32
            } else if kind != .webp {
                noteLabel.stringValue = "\(ImageFormats.shortName(type)) has no lossless mode on this Mac."
                place(noteLabel, x, y, 272, 16)
                y += 24
            }
        case .png:
            label("Compression:", y)
            place(levelSlider, x, y, 200, 24)
            levelSlider.numberOfTickMarks = 10
            levelSlider.allowsTickMarkValuesOnly = true
            levelSlider.setAccessibilityLabel("Compression level")
            levelSlider.target = self
            levelSlider.action = #selector(controlChanged(_:))
            place(levelValue, x + 208, y + 4, 60, 16)
            y += 28
            let hint = Labels.label("0 stores the pixels; 9 is smallest. Every level keeps every pixel.", size: 11)
            place(hint, x, y, 272, 30)
            hint.lineBreakMode = .byWordWrapping
            hint.maximumNumberOfLines = 2
            y += 36
        case .tiff:
            label("Compression:", y)
            place(tiffPopup, x, y - 2, 160, 28)
            tiffPopup.setAccessibilityLabel("Compression")
            tiffPopup.target = self
            tiffPopup.action = #selector(controlChanged(_:))
            y += 32
        case .gif:
            ditherBox.font = Theme.font(12)
            ditherBox.target = self
            ditherBox.action = #selector(controlChanged(_:))
            place(ditherBox, x, y, 200)
            y += 32
        }
        if kind.holdsMetadata {
            y += 8
            section("Metadata", y)
            y += 24
            label("Metadata:", y)
            place(metadataPopup, x, y - 2, 260, 28)
            metadataPopup.setAccessibilityLabel("Metadata")
            metadataPopup.target = self
            metadataPopup.action = #selector(controlChanged(_:))
            y += 30
            if !hasMetadata {
                let none = Labels.label("This image has no EXIF, XMP or GPS metadata.", size: 11)
                place(none, x, y, 272, 16)
                y += 24
            }
        }
        y += 8
        place(sizeLabel, 16, y, 408, 18)
        sizeLabel.setAccessibilityLabel("File size")
        y += 26
        if kind.isLossy {
            previewFrame.wantsLayer = true
            previewFrame.layer?.borderWidth = 1
            previewFrame.layer?.borderColor = Theme.line.cgColor
            place(previewFrame, 16, y, 408, 200)
            previewView.imageScaling = .scaleProportionallyUpOrDown
            previewView.setAccessibilityLabel("Preview of the saved image")
            previewView.frame = NSRect(x: 1, y: 1, width: 406, height: 198)
            previewFrame.addSubview(previewView)
            y += 212
        }
        cancelButton = NSButton(title: "Cancel", target: self, action: #selector(cancel(_:)))
        cancelButton.keyEquivalent = "\u{1b}"
        okButton = NSButton(title: "OK", target: self, action: #selector(ok(_:)))
        okButton.keyEquivalent = "\r"
        place(cancelButton, Self.width - 16 - 80 - 4 - 80, y, 80, 28)
        place(okButton, Self.width - 16 - 80, y, 80, 28)
        return y + 28 + 16
    }

    /// Copies the options into the controls.
    func show() {
        let o = options
        let q = kind == .jpeg ? o.jpegQuality : o.quality
        qualitySlider.integerValue = q
        if qualityField.currentEditor() == nil { qualityField.stringValue = String(q) }
        subsamplingPopup.selectItem(at: ChromaSubsampling.allCases.firstIndex(of: o.jpegSubsampling) ?? 0)
        levelSlider.integerValue = o.pngLevel
        levelValue.stringValue = String(o.pngLevel)
        tiffPopup.selectItem(at: TIFFCompression.allCases.firstIndex(of: o.tiffCompression) ?? 0)
        ditherBox.state = o.gifDither ? .on : .off
        metadataPopup.selectItem(at: MetadataChoice.allCases.firstIndex(of: o.metadata) ?? 0)
    }

    func show(_ r: SaveSizePreview.Result) {
        if let bytes = r.bytes {
            shownBytes = bytes
            sizeLabel.stringValue = "File size: \(FileSizeText.text(bytes))"
            sizeLabel.textColor = Theme.ink
        } else {
            shownBytes = nil
            sizeLabel.stringValue = "Preview failed: \(r.error ?? "unknown error")"
            sizeLabel.textColor = NSColor.systemRed
        }
        if let image = r.image {
            previewView.image = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
        }
    }

    /// Changes the options (also used by tests), shows them and measures again.
    public func update(_ body: (inout SaveOptions) -> Void) {
        body(&options)
        options = options.clamped(for: type)
        show()
        sizeLabel.stringValue = "Measuring…"
        sizeLabel.textColor = Theme.muted
        preview.request(options)
    }

    @objc private func controlChanged(_ sender: Any?) {
        update { o in
            if kind == .jpeg { o.jpegQuality = qualitySlider.integerValue } else { o.quality = qualitySlider.integerValue }
            o.jpegSubsampling = ChromaSubsampling.allCases[max(subsamplingPopup.indexOfSelectedItem, 0)]
            o.pngLevel = levelSlider.integerValue
            o.tiffCompression = TIFFCompression.allCases[max(tiffPopup.indexOfSelectedItem, 0)]
            o.gifDither = ditherBox.state == .on
            o.metadata = MetadataChoice.allCases[max(metadataPopup.indexOfSelectedItem, 0)]
        }
        // show() leaves a field being typed in alone; a slider move must still
        // reach the focused field, or Return (the field's action) would put the
        // old number back (M28).
        if (sender as AnyObject?) === qualitySlider, let editor = qualityField.currentEditor() {
            editor.string = String(kind == .jpeg ? options.jpegQuality : options.quality)
        }
    }

    private func applyField() {
        guard let v = Int(qualityField.stringValue.trimmingCharacters(in: .whitespaces)) else { return }
        update { o in if kind == .jpeg { o.jpegQuality = v } else { o.quality = v } }
    }

    public func controlTextDidChange(_ note: Notification) { applyField() }

    @objc private func fieldTyped(_ sender: NSTextField) {
        applyField()
        window?.makeFirstResponder(nil)
        show()
    }

    public func present(on parent: NSWindow?) {
        if let window { L10n.localizeDialog(window) }  // M59: translated and named before it shows
        if let parent {
            parent.beginSheet(window!) { _ in }
        } else {
            window?.center()
            window?.makeKeyAndOrderFront(nil)
        }
    }

    @objc public func ok(_ sender: Any?) { close(options) }
    @objc public func cancel(_ sender: Any?) { close(nil) }

    private func close(_ result: SaveOptions?) {
        preview.cancel()
        if let sheet = window, let parent = sheet.sheetParent { parent.endSheet(sheet) }
        window?.orderOut(nil)
        let done = onClose
        onClose = nil
        done?(result)
    }
}
