// M46 Text tool: CoreText layout (clusters, carets, bidi, emoji, fallback,
// alignment), the NSTextInputClient flow (composition, candidates, Enter and
// Escape rules) and the engine session (one step on Finish, preview equals
// commit, Cancel changes nothing). Input methods are driven through the
// NSTextInputClient calls they make, in process (no XCUITest on this Mac).
import AppKit
@testable import OpenPainterKit
import XCTest

@MainActor
final class TextLayoutTests: XCTestCase {
    func style(_ family: String = "Helvetica Neue", size: Double = 24, align: TextStyleSpec.Alignment = .left)
        -> TextStyleSpec
    {
        TextStyleSpec(family: family, pointSize: size, alignment: align)
    }

    func testUTF16ToRustByteOffsets() {
        let s = "a\u{E9}😀e\u{301}"  // a, é (2 bytes), 😀 (2 UTF-16, 4 bytes), e + combining acute (1 + 2 bytes)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 0, in: s), 0)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 1, in: s), 1)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 2, in: s), 3)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 3, in: s), 3, "inside the surrogate pair rounds down")
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 4, in: s), 7)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 5, in: s), 8)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 6, in: s), 10)
        XCTAssertEqual(TextLayoutResult.utf8Offset(forUTF16: 99, in: s), s.utf8.count)
    }

    func testCombiningAccentsAreOneCluster() {
        let s = "e\u{301}\u{323}x"  // e + acute + dot below: one cluster, then x
        let l = TextLayoutEngine.layout(s, style: style())
        XCTAssertEqual(l.clusters.map(\.range), [NSRange(location: 0, length: 3), NSRange(location: 3, length: 1)])
        XCTAssertEqual(l.carets.map(\.offset), [0, 3, 4], "no caret inside the cluster")
        XCTAssertGreaterThan(l.clusters[0].bounds.width, 0)
        var m = TextEditingModel(text: s)
        m.moveBackward()
        XCTAssertEqual(m.caret, 3)
        m.moveBackward()
        XCTAssertEqual(m.caret, 0, "one step over the whole cluster")
        m.moveForward()
        XCTAssertEqual(m.caret, 3)
        m.deleteBackward()
        XCTAssertEqual(m.text, "x", "Backspace removes the whole cluster")
        var flag = TextEditingModel(text: "🇯🇵👍🏽", caret: 0)
        flag.deleteForward()
        XCTAssertEqual(flag.text, "👍🏽", "a flag is one cluster")
        flag.moveForward()
        XCTAssertEqual(flag.caret, 4, "a skin-tone emoji is one cluster")
    }

    func testArabicCaretOrderAndClusterBounds() {
        let arabic = "مرحبا"
        let l = TextLayoutEngine.layout(arabic, style: style())
        let xs = l.carets.map(\.x)
        XCTAssertEqual(l.carets.map(\.offset), Array(0...5))
        for i in 1..<xs.count {
            XCTAssertLessThan(xs[i], xs[i - 1], "right-to-left: each later caret is further left (\(xs))")
        }
        XCTAssertTrue(l.clusters.allSatisfy(\.rightToLeft))
        for i in 1..<l.clusters.count {
            XCTAssertLessThanOrEqual(
                l.clusters[i].bounds.maxX, l.clusters[i - 1].bounds.minX + 0.5,
                "each later cluster sits left of the one before")
        }
        XCTAssertFalse(l.runs.isEmpty)
        XCTAssertTrue(l.runFonts.contains { $0 != l.fontName }, "Arabic comes from a cascade font: \(l.runFonts)")

        // Mixed: a left-to-right paragraph with an Arabic word inside.
        let mixed = "ab مرح cd"
        let m = TextLayoutEngine.layout(mixed, style: style())
        func caret(_ o: Int) -> Double { m.caret(at: o).x }
        XCTAssertLessThan(caret(0), caret(1))
        XCTAssertLessThan(caret(1), caret(2))
        XCTAssertGreaterThan(caret(4), caret(5), "inside the Arabic word the caret moves left")
        XCTAssertGreaterThan(caret(5), caret(6))
        XCTAssertLessThan(caret(8), caret(9), "back to left to right after it")
        let arabicClusters = m.clusters.filter { (3..<6).contains($0.range.location) }
        XCTAssertEqual(arabicClusters.count, 3)
        XCTAssertTrue(arabicClusters.allSatisfy(\.rightToLeft))
        XCTAssertFalse(m.clusters[0].rightToLeft)
        let latin = m.clusters.filter { !$0.rightToLeft && $0.range.length == 1 && m.text.utf16.count > 0 }
        XCTAssertTrue(latin.allSatisfy { $0.bounds.width >= 0 })
        XCTAssertLessThan(m.clusters[1].bounds.maxX, arabicClusters.map(\.bounds.minX).min()! + 0.5)
    }

    func testEmojiIsAColorRun() throws {
        let l = TextLayoutEngine.layout("A😀", style: style(size: 36))
        let color = try XCTUnwrap(l.runs.first { $0.kind == .color }, "the emoji is a color run")
        let coverage = try XCTUnwrap(l.runs.first { $0.kind == .coverage }, "the letter is a coverage run")
        XCTAssertEqual(color.bytes.count, color.width * color.height * 4)
        var colored = 0
        for i in stride(from: 0, to: color.bytes.count, by: 4) {
            let (r, g, b, a) = (color.bytes[i], color.bytes[i + 1], color.bytes[i + 2], color.bytes[i + 3])
            XCTAssertTrue(r <= a && g <= a && b <= a, "premultiplied")
            if a > 200, Int(r) - Int(b) > 60 { colored += 1 }
        }
        XCTAssertGreaterThan(colored, 20, "the emoji keeps its own colors")
        let emoji = try XCTUnwrap(l.clusters.last)
        XCTAssertEqual(emoji.range, NSRange(location: 1, length: 2))
        XCTAssertLessThanOrEqual(CGFloat(coverage.x + coverage.width), emoji.bounds.minX + 2,
                                 "the coverage run holds the letter only")
    }

    func testMissingFontFallsBackWithAReportedName() {
        let l = TextLayoutEngine.layout("Hello", style: style("No Such Font OP46"))
        XCTAssertEqual(l.missingFont, "No Such Font OP46")
        XCTAssertTrue(l.fontName.hasPrefix("Helvetica"), l.fontName)
        XCTAssertFalse(l.runs.isEmpty, "the text still renders")
        var pinned = style()
        pinned.postScriptName = "NoSuchFace-Regular"
        XCTAssertEqual(TextLayoutEngine.layout("x", style: pinned).missingFont, "NoSuchFace-Regular")
        XCTAssertNil(TextLayoutEngine.layout("x", style: style()).missingFont)
    }

    func testMultilineAlignment() {
        let s = "ab\nabcdef\n"
        for align in TextStyleSpec.Alignment.allCases {
            let l = TextLayoutEngine.layout(s, style: style(align: align))
            XCTAssertEqual(l.lines.count, 3)
            XCTAssertEqual(l.lines.map(\.range), [NSRange(location: 0, length: 2), NSRange(location: 3, length: 6),
                                                  NSRange(location: 10, length: 0)])
            XCTAssertEqual(l.lines[1].top, l.lines[0].top + l.lines[0].height)
            XCTAssertGreaterThan(l.lines[1].width, l.lines[0].width)
            for line in l.lines {
                switch align {
                case .left: XCTAssertEqual(line.x, 0)
                case .center: XCTAssertEqual(line.x + line.width / 2, 0, accuracy: 1e-9)
                case .right: XCTAssertEqual(line.x + line.width, 0, accuracy: 1e-9)
                }
            }
            // The caret after the last line break is on the empty third line.
            XCTAssertEqual(l.caret(at: 10).line, 2)
            XCTAssertEqual(l.caret(at: 2).line, 0, "the caret before a line break stays on its line")
            let run = l.runs[0]
            XCTAssertGreaterThan(Double(run.y + run.height), l.lines[1].baseline, "the raster covers both lines")
        }
    }

    func testStylesChangeTheRasterAndAntialiasOffIsTwoLevel() {
        let plain = TextLayoutEngine.layout("Text", style: style())
        var s = style()
        s.underline = true
        let under = TextLayoutEngine.layout("Text", style: s)
        XCTAssertGreaterThan(under.runs[0].bytes.map(Int.init).reduce(0, +), plain.runs[0].bytes.map(Int.init).reduce(0, +))
        s = style()
        s.strikethrough = true
        XCTAssertNotEqual(TextLayoutEngine.layout("Text", style: s).runs, plain.runs)
        s = style()
        s.bold = true
        let bold = TextLayoutEngine.layout("Text", style: s)
        XCTAssertTrue(bold.fontName.contains("Bold"), bold.fontName)
        s = style()
        s.italic = true
        XCTAssertTrue(TextLayoutEngine.layout("Text", style: s).fontName.contains("Italic"))
        s = style()
        s.antialias = false
        let sharp = TextLayoutEngine.layout("Text", style: s)
        XCTAssertTrue(sharp.runs[0].bytes.allSatisfy { $0 == 0 || $0 == 255 })
        XCTAssertTrue(plain.runs[0].bytes.contains { $0 > 0 && $0 < 255 })
    }

    /// Mac rendering golden. It pins the font by PostScript name and the OS
    /// build; on another build it is skipped with the reason printed.
    func testCoreTextGoldenHelveticaNeue24OnBuild26A434() throws {
        let build = Self.osBuild()
        guard build == "26A434" else {
            print("SKIP testCoreTextGoldenHelveticaNeue24OnBuild26A434: golden recorded on OS build 26A434, this is \(build)")
            throw XCTSkip("golden recorded on OS build 26A434, this Mac runs \(build)")
        }
        var s = TextStyleSpec(postScriptName: "HelveticaNeue", pointSize: 24)
        let l = TextLayoutEngine.layout("Ag fi", style: s)
        XCTAssertEqual(l.fontName, "HelveticaNeue")
        XCTAssertEqual(Self.digest(l.runs), "40cc44a121136958", "antialiased golden")
        s.antialias = false
        XCTAssertEqual(Self.digest(TextLayoutEngine.layout("Ag fi", style: s).runs), "a8edbeb0708876bd", "sharp golden")
    }

    static func osBuild() -> String {
        var size = 0
        sysctlbyname("kern.osversion", nil, &size, nil, 0)
        var buf = [CChar](repeating: 0, count: size)
        sysctlbyname("kern.osversion", &buf, &size, nil, 0)
        return String(cString: buf)
    }

    /// FNV-1a 64 over each run's kind, position, size and bytes.
    static func digest(_ runs: [TextLayoutResult.Run]) -> String {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        func eat(_ bytes: [UInt8]) {
            for b in bytes {
                h ^= UInt64(b)
                h = h &* 0x0100_0000_01b3
            }
        }
        for r in runs {
            eat(Array("\(r.kind.rawValue) \(r.x) \(r.y) \(r.width) \(r.height)".utf8))
            eat(r.bytes)
        }
        return String(format: "%016llx", h)
    }
}

@MainActor
final class TextToolTests: XCTestCase {
    var fx: ShellFixture!

    override func setUp() async throws {
        fx = try ShellFixture()
    }

    override func tearDown() async throws {
        fx.tearDown()
    }

    // MARK: Helpers

    /// Opens an image with the Text tool chosen.
    func openText() throws -> EditorWindowController {
        let wc = try fx.open()
        fx.workspace.tools.tool = .text
        fx.workspace.tools.settings.text.size = 24
        XCTAssertEqual(wc.canvas.tool, .click)
        return wc
    }

    /// Waits until the engine has run every command sent so far.
    func settle(_ wc: EditorWindowController) async throws {
        let id = wc.textTool.isEditing ? "tool.text.update" : "tool.select"
        let event = try await wc.editor.document.run(id, [:])
        XCTAssertEqual(event.status, "ok", "\(id) barrier")
        wc.editor.refresh()
    }

    func pixels(_ wc: EditorWindowController, preview: Bool = false) throws -> [UInt8] {
        try wc.editor.document.pixels(preview: preview)
    }

    var input: TextInputView!

    func start(_ wc: EditorWindowController, at p: CGPoint = CGPoint(x: 40, y: 60)) {
        wc.click(p)
        XCTAssertTrue(wc.textTool.isEditing)
        input = wc.textTool.input
        XCTAssertFalse(input.isHidden)
        XCTAssertTrue(wc.window?.firstResponder === input, "the input view takes the keys")
    }

    let none = NSRange(location: NSNotFound, length: 0)

    // MARK: Tests

    func testJapaneseCompositionThenConfirm() async throws {
        let wc = try openText()
        let original = try pixels(wc)
        let steps = wc.editor.info.historyLabels.count
        start(wc)
        // Romaji typed into the Japanese input method: the composing kana are marked.
        input.setMarkedText("に", selectedRange: NSRange(location: 1, length: 0), replacementRange: none)
        input.setMarkedText("にほん", selectedRange: NSRange(location: 3, length: 0), replacementRange: none)
        XCTAssertTrue(input.hasMarkedText())
        XCTAssertEqual(input.markedRange(), NSRange(location: 0, length: 3))
        XCTAssertEqual(input.selectedRange(), NSRange(location: 3, length: 0))
        XCTAssertEqual(input.markedUnderlines.all.count, 1, "the composing text is underlined")
        try await settle(wc)
        XCTAssertNotEqual(try pixels(wc, preview: true), original, "the composing text shows in the preview")
        // Conversion picks kanji and the input method confirms.
        input.insertText("日本", replacementRange: none)
        XCTAssertFalse(input.hasMarkedText())
        XCTAssertEqual(wc.textTool.model.text, "日本")
        XCTAssertEqual(wc.textTool.model.caret, 2)
        input.insertText("語", replacementRange: none)
        XCTAssertEqual(wc.textTool.model.text, "日本語")
        try await settle(wc)
        let preview = try pixels(wc, preview: true)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps, "no step while typing")
        wc.textTool.finish()
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 1, "one step on Finish")
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Text")
        XCTAssertEqual(try pixels(wc), preview, "preview equals commit")
        XCTAssertFalse(wc.textTool.isEditing)
        XCTAssertTrue(wc.window?.firstResponder === wc.canvas)
    }

    func testChineseCandidateConfirmationAndCandidateWindowAtTheCaretAtAnyZoom() async throws {
        let wc = try openText()
        start(wc, at: CGPoint(x: 100, y: 80))
        input.insertText("你", replacementRange: none)
        // Pinyin: "zhongguo" composes; the input method asks where to put its candidate window.
        input.setMarkedText("zhongguo", selectedRange: NSRange(location: 8, length: 0), replacementRange: none)
        for zoom in [0.25, 1.0, 4.0] {
            var v = wc.canvas.viewport
            v.setZoom(zoom)
            wc.canvas.setViewport(v)
            let layout = try XCTUnwrap(wc.textTool.layout)
            let caret = layout.caret(at: input.selectedRange().location)
            // Expected: the caret, anchored at the click, through the viewport, then to the screen.
            let a = wc.canvas.viewport.viewPoint(
                ofDocument: CGPoint(x: wc.textTool.anchor.x + caret.x, y: wc.textTool.anchor.y + caret.top))
            let expected = wc.window!.convertToScreen(
                input.convert(CGRect(x: a.x, y: a.y, width: 1, height: caret.height * zoom), to: nil))
            var actual = NSRange()
            let rect = input.firstRect(forCharacterRange: input.selectedRange(), actualRange: &actual)
            XCTAssertEqual(rect.minX, expected.minX, accuracy: 0.01, "zoom \(zoom)")
            XCTAssertEqual(rect.minY, expected.minY, accuracy: 0.01, "zoom \(zoom)")
            XCTAssertEqual(rect.height, CGFloat(caret.height * zoom), accuracy: 0.01, "zoom \(zoom)")
            XCTAssertEqual(actual, input.selectedRange())
            // A range on the marked text spans its clusters.
            let whole = input.firstRect(forCharacterRange: input.markedRange(), actualRange: nil)
            XCTAssertGreaterThan(whole.width, CGFloat(10 * zoom))
        }
        // The user steps to a candidate: the input method shows it with its selected segment.
        input.setMarkedText("中国", selectedRange: NSRange(location: 0, length: 2), replacementRange: none)
        XCTAssertEqual(input.selectedRange(), NSRange(location: 1, length: 2))
        XCTAssertEqual(input.markedUnderlines.selected.count, 1, "the chosen segment has the thick underline")
        input.insertText("中国", replacementRange: none)
        XCTAssertEqual(wc.textTool.model.text, "你中国")
        XCTAssertFalse(input.hasMarkedText())
        XCTAssertEqual(input.characterIndex(for: .zero), input.characterIndex(for: .zero))  // callable
        XCTAssertEqual(input.attributedSubstring(forProposedRange: NSRange(location: 1, length: 2), actualRange: nil)?.string,
                       "中国")
        wc.textTool.finish()
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Text")
    }

    func testEnterConfirmsMarkedTextBeforeFinishAndThenMakesNewLines() async throws {
        let wc = try openText()
        let steps = wc.editor.info.historyLabels.count
        start(wc)
        input.setMarkedText("か", selectedRange: NSRange(location: 1, length: 0), replacementRange: none)
        input.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        XCTAssertFalse(input.hasMarkedText(), "Enter confirmed the composing text")
        XCTAssertEqual(wc.textTool.model.text, "か", "and added no line")
        XCTAssertTrue(wc.textTool.isEditing, "and did not finish")
        input.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        input.insertText("b", replacementRange: none)
        XCTAssertEqual(wc.textTool.model.text, "か\nb", "Return makes a new line when nothing composes")
        XCTAssertEqual(wc.textTool.layout?.lines.count, 2)
        XCTAssertTrue(wc.textTool.isEditing)
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps)
        // The canvas Enter path (window focus rules) follows the same rule.
        input.setMarkedText("x", selectedRange: NSRange(location: 1, length: 0), replacementRange: none)
        wc.canvasKey(.enter)
        XCTAssertEqual(wc.textTool.model.text, "か\nbx")
        // Command-Return finishes.
        input.keyDown(with: keyEvent("\r", keyCode: 36, modifiers: .command, window: wc.window))
        XCTAssertFalse(wc.textTool.isEditing)
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 1)
    }

    func testEscapeDuringCompositionCancelsOnlyTheComposition() async throws {
        let wc = try openText()
        start(wc)
        input.insertText("ab", replacementRange: none)
        input.setMarkedText("にほ", selectedRange: NSRange(location: 2, length: 0), replacementRange: none)
        input.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        XCTAssertFalse(input.hasMarkedText())
        XCTAssertEqual(wc.textTool.model.text, "ab")
        XCTAssertEqual(wc.textTool.model.caret, 2)
        XCTAssertTrue(wc.textTool.isEditing, "the text is still open")
    }

    func testFocusLossConfirmsCompositionAndKeepsTheTextOpen() async throws {
        let wc = try openText()
        start(wc)
        input.setMarkedText("にほ", selectedRange: NSRange(location: 2, length: 0), replacementRange: none)
        // Focus moves to an options-row field (or another window) while composing.
        wc.window?.makeFirstResponder(wc.optionsBar.textControls.size)
        XCTAssertFalse(input.hasMarkedText(), "losing focus confirms the composition")
        XCTAssertEqual(wc.textTool.model.text, "にほ")
        XCTAssertTrue(wc.textTool.isEditing, "the text stays open")
        // A click in the text takes the keys back.
        input.handleClick(atView: input.viewRect(CGRect(x: 1, y: 2, width: 0, height: 0)).origin)
        XCTAssertTrue(wc.window?.firstResponder === input)
        wc.textTool.cancel()
    }

    /// paint.net 5 (https://www.getpaint.net/doc/latest/TextTool.html) and OpenPDN
    /// 3.36: with nothing composing, Escape finishes the text.
    func testEscapeOutsideCompositionFinishesTheText() async throws {
        let wc = try openText()
        let steps = wc.editor.info.historyLabels.count
        start(wc)
        input.insertText("Kept", replacementRange: none)
        try await settle(wc)
        let preview = try pixels(wc, preview: true)
        input.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        XCTAssertFalse(wc.textTool.isEditing, "Escape ends the text")
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 1, "one step")
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Text")
        XCTAssertEqual(try pixels(wc), preview, "the text is committed as previewed")
        // Escape through the window's focus rules does the same.
        start(wc, at: CGPoint(x: 200, y: 200))
        input.insertText("x", replacementRange: none)
        wc.canvasKey(.escape)
        XCTAssertFalse(wc.textTool.isEditing)
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 2)
        // Escape on empty text commits nothing.
        start(wc)
        input.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        XCTAssertFalse(wc.textTool.isEditing)
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 2)
    }

    /// Cancel (no longer on a key) leaves the image and its history as they were.
    func testCancelLeavesBytesAndHistoryUnchanged() async throws {
        let wc = try openText()
        let original = try pixels(wc)
        let info = wc.editor.info
        start(wc)
        input.insertText("Gone", replacementRange: none)
        try await settle(wc)
        XCTAssertNotEqual(try pixels(wc, preview: true), original)
        wc.textTool.cancel()
        XCTAssertFalse(wc.textTool.isEditing)
        try await settle(wc)
        XCTAssertEqual(try pixels(wc), original, "bytes unchanged")
        XCTAssertEqual(try pixels(wc, preview: true), original, "no preview left")
        XCTAssertEqual(wc.editor.info.historyLabels, info.historyLabels, "history unchanged")
        XCTAssertEqual(wc.editor.info.historyIndex, info.historyIndex)
        XCTAssertEqual(wc.editor.info.dirty, info.dirty)
    }

    func testCaretMovesAndDeletesByClusterThroughKeyCommands() async throws {
        let wc = try openText()
        start(wc)
        input.insertText("ae\u{301}👍🏽\nxy", replacementRange: none)
        let m = { wc.textTool.model }
        XCTAssertEqual(m().caret, 10)
        input.keyDown(with: keyEvent(String(Character(UnicodeScalar(NSHomeFunctionKey)!)), keyCode: 115, window: wc.window))
        XCTAssertEqual(m().caret, 8, "Home: start of the line")
        input.doCommand(by: #selector(NSResponder.moveLeft(_:)))
        XCTAssertEqual(m().caret, 7, "before the line break")
        input.doCommand(by: #selector(NSResponder.moveLeft(_:)))
        XCTAssertEqual(m().caret, 3, "over the whole emoji with its skin tone")
        input.doCommand(by: #selector(NSResponder.moveLeft(_:)))
        XCTAssertEqual(m().caret, 1, "over e with its accent")
        input.doCommand(by: #selector(NSResponder.moveRight(_:)))
        XCTAssertEqual(m().caret, 3)
        input.doCommand(by: #selector(NSResponder.deleteForward(_:)))
        XCTAssertEqual(m().text, "ae\u{301}\nxy")
        input.doCommand(by: #selector(NSResponder.deleteBackward(_:)))
        XCTAssertEqual(m().text, "a\nxy")
        input.keyDown(with: keyEvent(String(Character(UnicodeScalar(NSEndFunctionKey)!)), keyCode: 119, window: wc.window))
        XCTAssertEqual(m().caret, 1, "End: before the line break")
        input.doCommand(by: #selector(NSResponder.moveDown(_:)))
        XCTAssertEqual(m().layoutLine(wc), 1, "Down moves to the next line")
        input.doCommand(by: #selector(NSResponder.moveUp(_:)))
        XCTAssertEqual(m().layoutLine(wc), 0)
        wc.textTool.cancel()
    }

    func testClickOutsideAndToolSwitchFinish() async throws {
        let wc = try openText()
        let steps = wc.editor.info.historyLabels.count
        start(wc, at: CGPoint(x: 20, y: 20))
        input.insertText("One", replacementRange: none)
        let firstAnchor = wc.textTool.anchor
        // A click inside the text moves the caret.
        let inside = input.viewRect(CGRect(x: 1, y: 2, width: 0, height: 0)).origin
        input.handleClick(atView: inside)
        XCTAssertEqual(wc.textTool.model.caret, 0)
        XCTAssertEqual(wc.textTool.anchor, firstAnchor)
        // A click far outside finishes and starts new text there.
        let outside = wc.canvas.viewport.viewPoint(ofDocument: CGPoint(x: 300, y: 300))
        input.handleClick(atView: outside)
        XCTAssertTrue(wc.textTool.isEditing)
        XCTAssertEqual(wc.textTool.anchor, CGPoint(x: 300, y: 300))
        XCTAssertEqual(wc.textTool.model.text, "")
        input.insertText("Two", replacementRange: none)
        fx.workspace.tools.tool = .paintbrush
        XCTAssertFalse(wc.textTool.isEditing, "another tool finishes")
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 2, "one step for each text")
        // Empty text adds no step.
        fx.workspace.tools.tool = .text
        start(wc)
        wc.textTool.finish()
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 2)
    }

    func testOptionsRowStylesTheOpenTextAndMissingFontIsReported() async throws {
        let wc = try openText()
        let bar = wc.optionsBar.textControls
        XCTAssertNotNil(bar.font.superview, "the Text options show")
        XCTAssertNotNil(bar.finish.superview)
        XCTAssertNotNil(wc.optionsBar.antialias.superview)
        XCTAssertNotNil(wc.optionsBar.blend.superview)
        XCTAssertGreaterThan(bar.font.numberOfItems, 10, "installed families")
        start(wc)
        input.insertText("Style", replacementRange: none)
        let plain = try XCTUnwrap(wc.textTool.layout)
        bar.bold.performClick(nil)
        XCTAssertTrue(fx.workspace.tools.settings.text.bold)
        XCTAssertTrue(bar.bold.isOn)
        XCTAssertTrue(try XCTUnwrap(wc.textTool.layout).fontName.contains("Bold"), "the open text restyles")
        bar.alignments[.right]!.performClick(nil)
        XCTAssertEqual(fx.workspace.tools.settings.text.alignment, .right)
        XCTAssertEqual(try XCTUnwrap(wc.textTool.layout).lines[0].x + wc.textTool.layout!.lines[0].width, 0,
                       accuracy: 1e-9)
        bar.size.stringValue = "48"
        bar.size.sendAction(bar.size.action, to: bar.size.target)
        XCTAssertEqual(fx.workspace.tools.settings.text.size, 48)
        XCTAssertGreaterThan(wc.textTool.layout!.lines[0].height, plain.lines[0].height * 1.5)
        fx.workspace.tools.settings.text.family = "No Such Font OP46"
        XCTAssertEqual(wc.textTool.layout?.missingFont, "No Such Font OP46")
        XCTAssertTrue(wc.statusBar.hint.stringValue.contains("not installed"), wc.statusBar.hint.stringValue)
        XCTAssertEqual(bar.font.titleOfSelectedItem, "No Such Font OP46")
        bar.finish.performClick(nil)
        XCTAssertFalse(wc.textTool.isEditing, "Finish commits")
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.last, "Text")
        XCTAssertEqual(wc.editor.info.historyCommands.last, "tool.text.finish")
    }

    func testBlendAndPrimaryColorReachTheEngineAndUndoKeepsTheText() async throws {
        let wc = try openText()
        let tools = fx.workspace.tools
        tools.primary = DocColor(r: 0, g: 200, b: 0)
        tools.settings.blend = .overwrite
        tools.settings.text.size = 60
        start(wc, at: CGPoint(x: 10, y: 10))
        input.insertText("█", replacementRange: none)  // a full block: solid pixels
        try await settle(wc)
        let layout = try XCTUnwrap(wc.textTool.layout)
        let run = try XCTUnwrap(layout.runs.first)
        // A pixel well inside the block.
        let w = wc.editor.info.width
        let x = Int(wc.textTool.anchor.x) + run.x + run.width / 2
        let y = Int(wc.textTool.anchor.y) + run.y + run.height / 2
        let preview = try pixels(wc, preview: true)
        XCTAssertEqual(Array(preview[(y * w + x) * 4..<(y * w + x) * 4 + 4]), [0, 200, 0, 255])
        // Undo while typing commits first, so Redo brings the text back.
        let steps = wc.editor.info.historyLabels.count
        wc.undo()
        XCTAssertFalse(wc.textTool.isEditing)
        try await settle(wc)
        XCTAssertEqual(wc.editor.info.historyLabels.count, steps + 1)
        XCTAssertEqual(wc.editor.info.historyIndex, steps - 1, "the Text step is undone")
        wc.redo()
        try await settle(wc)
        XCTAssertEqual(try pixels(wc), preview)
    }
}

@MainActor
extension TextToolTests {
    /// Writes PNGs for a visual check when OP_PANEL_PNG_DIR is set: the Text
    /// options row in light and dark, the canvas preview with composing text
    /// and the edit overlay, and the golden layout's coverage.
    func testRenderTextToolPNGs() async throws {
        let wc = try openText()
        let bar = wc.optionsBar
        bar.layoutSubtreeIfNeeded()
        XCTAssertEqual(bar.frame.height, Theme.optionsHeight)
        for v in [bar.textControls.bold!, bar.textControls.alignments[.left]!] {
            XCTAssertEqual(v.alignmentRect(forFrame: v.frame).size, NSSize(width: 28, height: 28), "28 pt targets")
        }
        XCTAssertEqual(bar.textControls.font.frame.height, 28)
        XCTAssertEqual(bar.textControls.size.frame.height, 28)
        guard let dir = ProcessInfo.processInfo.environment["OP_PANEL_PNG_DIR"] else { return }
        func write(_ data: Data?, _ name: String) throws {
            try XCTUnwrap(data).write(to: URL(fileURLWithPath: dir).appendingPathComponent("m46-\(name).png"))
        }
        func snap(_ view: NSView) throws -> Data? {
            view.needsLayout = true
            view.layoutSubtreeIfNeeded()
            for sub in view.subviewsDeep { sub.needsDisplay = true }
            let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
            view.cacheDisplay(in: view.bounds, to: rep)
            return rep.representation(using: .png, properties: [:])
        }
        fx.workspace.tools.settings.text.bold = true
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            wc.window?.appearance = NSAppearance(named: appearance)
            try write(snap(bar), "options-\(name)")
        }
        wc.window?.appearance = nil
        fx.workspace.tools.settings.text.bold = false
        fx.workspace.tools.settings.text.size = 36
        var v = wc.canvas.viewport
        v.setZoom(2)
        wc.canvas.setViewport(v)
        start(wc, at: CGPoint(x: 30, y: 30))
        input.insertText("Hé 😀 مرحبا\n", replacementRange: none)
        input.setMarkedText("にほんご", selectedRange: NSRange(location: 0, length: 2), replacementRange: none)
        try await settle(wc)
        let w = wc.editor.info.width, h = wc.editor.info.height
        var px = try pixels(wc, preview: true)
        for i in stride(from: 0, to: px.count, by: 4) where px[i + 3] < 255 {
            let a = Int(px[i + 3])
            for c in 0..<3 { px[i + c] = UInt8((Int(px[i + c]) * a + 255 * (255 - a)) / 255) }
            px[i + 3] = 255
        }
        let provider = CGDataProvider(data: Data(px) as CFData)!
        let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
        try write(NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]), "preview")
        try write(snap(input), "overlay")
        let golden = TextLayoutEngine.layout("Ag fi", style: TextStyleSpec(postScriptName: "HelveticaNeue", pointSize: 24))
        let run = golden.runs[0]
        let gray = CGImage(width: run.width, height: run.height, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: run.width,
                           space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: 0),
                           provider: CGDataProvider(data: Data(run.bytes) as CFData)!, decode: nil,
                           shouldInterpolate: false, intent: .defaultIntent)!
        try write(NSBitmapImageRep(cgImage: gray).representation(using: .png, properties: [:]), "golden-coverage")
        wc.textTool.cancel()
    }
}

extension TextEditingModel {
    @MainActor func layoutLine(_ wc: EditorWindowController) -> Int {
        wc.textTool.layout?.caret(at: caret).line ?? -1
    }
}
