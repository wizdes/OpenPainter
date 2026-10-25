// The custom controls of the effect dialogs (M24): a number row (slider and
// field), the angle dial, the point pad on an image thumbnail, the Curves
// editor and the Levels editor. Each reports changes through a closure; the
// dialog owns the values.
import AppKit

/// A slider and a number field for one value, kept in step with each other.
@MainActor
public final class NumberRow: NSView, NSTextFieldDelegate {
    public let slider: NSSlider?
    public let field: NSTextField
    public let minValue: Double?
    public let maxValue: Double?
    public let step: Double
    public let decimals: Int
    var onChange: ((Double) -> Void)?

    public init(min: Double?, max: Double?, step: Double, decimals: Int, width: CGFloat, fieldWidth: CGFloat = 64,
                slider useSlider: Bool = true, label: String) {
        minValue = min
        maxValue = max
        self.step = step
        self.decimals = decimals
        if useSlider, let min, let max, max > min {
            let s = NSSlider(value: min, minValue: min, maxValue: max, target: nil, action: nil)
            s.isContinuous = true
            s.setAccessibilityLabel(label)
            slider = s
        } else {
            slider = nil
        }
        field = NSTextField(string: "")
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: 28))
        let f = NumberFormatter()
        f.numberStyle = .decimal
        f.usesGroupingSeparator = false
        f.minimumFractionDigits = decimals
        f.maximumFractionDigits = decimals
        if let min { f.minimum = NSNumber(value: min) }
        if let max { f.maximum = NSNumber(value: max) }
        field.formatter = f
        field.font = Theme.font(12)
        field.alignment = .right
        field.delegate = self
        field.target = self
        field.action = #selector(typed(_:))
        field.setAccessibilityLabel(range(label))
        if let slider {
            slider.frame = NSRect(x: 0, y: 0, width: width - fieldWidth - 8, height: 28)
            slider.target = self
            slider.action = #selector(slid(_:))
            addSubview(slider)
            field.frame = NSRect(x: width - fieldWidth, y: 2, width: fieldWidth, height: 24)
        } else {
            field.frame = NSRect(x: 0, y: 2, width: fieldWidth, height: 24)
        }
        addSubview(field)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    private func range(_ label: String) -> String {
        guard let minValue, let maxValue else { return "\(label) value" }
        return "\(label) value, \(format(minValue)) to \(format(maxValue))"
    }

    func format(_ v: Double) -> String { String(format: "%.\(decimals)f", v) }

    /// Shows `v` without reporting a change. The field keeps what the user is
    /// typing.
    public func show(_ v: Double) {
        slider?.doubleValue = v
        // A field being typed in keeps its text while it already says `v`
        // ("12." while typing 12.5); any other value replaces it.
        if field.currentEditor() == nil || Double(field.stringValue).map(snap) != snap(v) {
            field.stringValue = format(v)
        }
    }

    func snap(_ v: Double) -> Double {
        var x = (v / step).rounded() * step
        if let minValue { x = Swift.max(x, minValue) }
        if let maxValue { x = Swift.min(x, maxValue) }
        return x
    }

    @objc func slid(_ sender: NSSlider) {
        let v = snap(sender.doubleValue)
        field.stringValue = format(v)
        onChange?(v)
    }

    @objc func typed(_ sender: NSTextField) {
        guard let v = Double(sender.stringValue) else { return }
        let x = snap(v)
        slider?.doubleValue = x
        onChange?(x)
    }

    public func controlTextDidChange(_ note: Notification) {
        guard let v = Double(field.stringValue) else { return }
        if let minValue, v < minValue { return }
        if let maxValue, v > maxValue { return }
        slider?.doubleValue = v
        onChange?(snap(v))
    }

    public func controlTextDidEndEditing(_ note: Notification) {
        if let v = Double(field.stringValue) { field.stringValue = format(snap(v)) }
    }
}

/// A round dial for an angle in degrees: 0 points right, positive angles turn
/// counter-clockwise, as in paint.net's angle chooser.
@MainActor
public final class AngleDial: NSView {
    public var degrees: Double = 0 { didSet { needsDisplay = true } }
    public let minValue: Double
    public let maxValue: Double
    var onChange: ((Double) -> Void)?

    public init(min: Double, max: Double) {
        minValue = min
        maxValue = max
        super.init(frame: NSRect(x: 0, y: 0, width: 56, height: 56))
        setAccessibilityRole(.slider)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    public override func draw(_ dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 2, dy: 2)
        let circle = NSBezierPath(ovalIn: r)
        Theme.field.setFill()
        circle.fill()
        Theme.panelLine.setStroke()
        circle.lineWidth = 1
        circle.stroke()
        let c = NSPoint(x: r.midX, y: r.midY)
        let a = degrees * .pi / 180
        let tip = NSPoint(x: c.x + cos(a) * r.width / 2, y: c.y - sin(a) * r.height / 2)
        let hand = NSBezierPath()
        hand.move(to: c)
        hand.line(to: tip)
        hand.lineWidth = 2
        Theme.accent.setStroke()
        hand.stroke()
        Theme.accent.setFill()
        NSBezierPath(ovalIn: NSRect(x: c.x - 3, y: c.y - 3, width: 6, height: 6)).fill()
        if window?.firstResponder === self {
            Theme.accent.setStroke()
            let ring = NSBezierPath(ovalIn: bounds.insetBy(dx: 0.5, dy: 0.5))
            ring.lineWidth = 1
            ring.stroke()
        }
    }

    /// The angle the point `p` (view coordinates) names, mapped into range.
    public func angle(at p: NSPoint) -> Double {
        let c = NSPoint(x: bounds.midX, y: bounds.midY)
        var a = atan2(-(p.y - c.y), p.x - c.x) * 180 / .pi  // -180...180
        if a < minValue { a += 360 }
        if a > maxValue { a -= 360 }
        return Swift.min(Swift.max(a.rounded(), minValue), maxValue)
    }

    public override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        track(event)
    }

    public override func mouseDragged(with event: NSEvent) { track(event) }

    private func track(_ event: NSEvent) {
        let a = angle(at: convert(event.locationInWindow, from: nil))
        guard a != degrees else { return }
        degrees = a
        onChange?(a)
    }

    public override func keyDown(with event: NSEvent) {
        let step: Double
        switch event.keyCode {
        case 123, 126: step = 1  // left, up: counter-clockwise
        case 124, 125: step = -1
        default: return super.keyDown(with: event)
        }
        degrees = Swift.min(Swift.max(degrees + step * (event.modifierFlags.contains(.shift) ? 15 : 1), minValue), maxValue)
        onChange?(degrees)
    }

    public override func accessibilityValue() -> Any? { "\(Int(degrees)) degrees" }
}

/// A small image thumbnail with a cross that marks a point. -1...1 spans the
/// image on each axis (y down); values outside the image clamp to its edge in
/// the drawing.
@MainActor
public final class PointPad: NSView {
    public var image: CGImage? { didSet { needsDisplay = true } }
    public var point = CGPoint.zero { didSet { needsDisplay = true } }
    public let range: (x: ClosedRange<Double>, y: ClosedRange<Double>)
    var onChange: ((CGPoint) -> Void)?

    public init(size: NSSize, x: ClosedRange<Double>, y: ClosedRange<Double>) {
        range = (x, y)
        super.init(frame: NSRect(origin: .zero, size: size))
        setAccessibilityRole(.button)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    /// Where the image draws: aspect fit in the bounds.
    var imageRect: NSRect {
        guard let image, image.width > 0, image.height > 0 else { return bounds.insetBy(dx: 1, dy: 1) }
        let b = bounds.insetBy(dx: 1, dy: 1)
        let s = min(b.width / CGFloat(image.width), b.height / CGFloat(image.height))
        let w = CGFloat(image.width) * s, h = CGFloat(image.height) * s
        return NSRect(x: b.midX - w / 2, y: b.midY - h / 2, width: w, height: h)
    }

    public override func draw(_ dirtyRect: NSRect) {
        Theme.field.setFill()
        bounds.fill()
        let r = imageRect
        if let image, let ctx = NSGraphicsContext.current?.cgContext {
            ctx.saveGState()
            ctx.translateBy(x: 0, y: r.maxY + r.minY)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(image, in: r)
            ctx.restoreGState()
        }
        Theme.panelLine.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()
        let p = location(of: point)
        let cross = NSBezierPath()
        cross.move(to: NSPoint(x: p.x - 6, y: p.y))
        cross.line(to: NSPoint(x: p.x + 6, y: p.y))
        cross.move(to: NSPoint(x: p.x, y: p.y - 6))
        cross.line(to: NSPoint(x: p.x, y: p.y + 6))
        cross.lineWidth = 3
        NSColor.white.setStroke()
        cross.stroke()
        cross.lineWidth = 1
        NSColor.black.setStroke()
        cross.stroke()
    }

    /// The view point for a value point.
    public func location(of v: CGPoint) -> NSPoint {
        let r = imageRect
        let x = min(max(v.x, -1), 1), y = min(max(v.y, -1), 1)
        return NSPoint(x: r.minX + (x + 1) / 2 * r.width, y: r.minY + (y + 1) / 2 * r.height)
    }

    /// The value point for a view point, clamped to the schema range.
    public func value(at p: NSPoint) -> CGPoint {
        let r = imageRect
        let x = Double((p.x - r.minX) / r.width) * 2 - 1
        let y = Double((p.y - r.minY) / r.height) * 2 - 1
        func q(_ v: Double, _ range: ClosedRange<Double>) -> Double {
            (min(max(v, range.lowerBound), range.upperBound) * 100).rounded() / 100
        }
        return CGPoint(x: q(x, range.x), y: q(y, range.y))
    }

    public override func mouseDown(with event: NSEvent) { track(event) }
    public override func mouseDragged(with event: NSEvent) { track(event) }

    private func track(_ event: NSEvent) {
        let v = value(at: convert(event.locationInWindow, from: nil))
        guard v != point else { return }
        point = v
        onChange?(v)
    }
}

// MARK: Curves

/// The Curves editor: control points on a 256 x 256 grid, one curve
/// (Luminosity) or three (RGB). Click adds a point and drags it; drag a point
/// to move it; Option-click or right-click removes it (the two end points
/// stay). Points never share an x.
@MainActor
public final class CurvesView: NSView {
    /// Points per channel, sorted by x, each [x, y] in 0...255.
    public private(set) var channels: [[[Int]]] = [[[0, 0], [255, 255]]]
    /// Which channel the mouse edits (0 for Luminosity; 0, 1, 2 for RGB).
    public var editing = 0 { didSet { needsDisplay = true } }
    var onChange: (([[[Int]]]) -> Void)?
    private var dragging: Int?

    public init() {
        super.init(frame: NSRect(x: 0, y: 0, width: 256, height: 256))
        setAccessibilityRole(.group)
        setAccessibilityLabel("Curve")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    /// Replaces the curves (sorted, duplicate x removed) without reporting.
    public func set(_ c: [[[Int]]]) {
        channels = c.map(Self.normalized)
        editing = min(editing, channels.count - 1)
        needsDisplay = true
    }

    static func normalized(_ points: [[Int]]) -> [[Int]] {
        var seen = Set<Int>()
        return points.map { [min(max($0[0], 0), 255), min(max($0[1], 0), 255)] }
            .sorted { $0[0] < $1[0] }
            .filter { seen.insert($0[0]).inserted }
    }

    // Grid coordinates: x right, y up (255 at the top).
    func grid(_ p: NSPoint) -> (Int, Int) {
        let x = Int((p.x / bounds.width * 255).rounded())
        let y = Int(((bounds.height - p.y) / bounds.height * 255).rounded())
        return (min(max(x, 0), 255), min(max(y, 0), 255))
    }

    func view(_ x: Int, _ y: Int) -> NSPoint {
        NSPoint(x: CGFloat(x) / 255 * bounds.width, y: bounds.height - CGFloat(y) / 255 * bounds.height)
    }

    /// Adds a point (or moves the one at that x) and reports. Returns its index.
    @discardableResult
    public func addPoint(x: Int, y: Int) -> Int {
        var pts = channels[editing].filter { $0[0] != x }
        pts.append([x, y])
        channels[editing] = Self.normalized(pts)
        report()
        return channels[editing].firstIndex { $0[0] == x } ?? 0
    }

    /// Moves point `i` of the edited channel, keeping it between its neighbors.
    public func movePoint(_ i: Int, x: Int, y: Int) {
        var pts = channels[editing]
        guard i < pts.count else { return }
        let lo = i > 0 ? pts[i - 1][0] + 1 : 0
        let hi = i + 1 < pts.count ? pts[i + 1][0] - 1 : 255
        pts[i] = [min(max(x, lo), hi), min(max(y, 0), 255)]
        guard pts != channels[editing] else { return }
        channels[editing] = pts
        report()
    }

    /// Removes point `i` unless it is an end point or only two remain.
    public func removePoint(_ i: Int) {
        let pts = channels[editing]
        guard pts.count > 2, i > 0, i < pts.count - 1 else { return }
        channels[editing].remove(at: i)
        report()
    }

    private func report() {
        needsDisplay = true
        onChange?(channels)
    }

    func hit(_ p: NSPoint) -> Int? {
        channels[editing].indices.first { i in
            let q = view(channels[editing][i][0], channels[editing][i][1])
            return abs(q.x - p.x) <= 6 && abs(q.y - p.y) <= 6
        }
    }

    public override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if let i = hit(p) {
            if event.modifierFlags.contains(.option) { removePoint(i); return }
            dragging = i
        } else {
            let (x, y) = grid(p)
            dragging = addPoint(x: x, y: y)
        }
    }

    public override func rightMouseDown(with event: NSEvent) {
        if let i = hit(convert(event.locationInWindow, from: nil)) { removePoint(i) }
    }

    public override func mouseDragged(with event: NSEvent) {
        guard let i = dragging else { return }
        let (x, y) = grid(convert(event.locationInWindow, from: nil))
        movePoint(i, x: x, y: y)
    }

    public override func mouseUp(with event: NSEvent) { dragging = nil }

    static let channelColors: [NSColor] = [.systemRed, .systemGreen, .systemBlue]

    public override func draw(_ dirtyRect: NSRect) {
        Theme.field.setFill()
        bounds.fill()
        Theme.line.setStroke()
        let grid = NSBezierPath()
        for i in 1..<4 {
            let t = bounds.width * CGFloat(i) / 4
            grid.move(to: NSPoint(x: t, y: 0))
            grid.line(to: NSPoint(x: t, y: bounds.height))
            grid.move(to: NSPoint(x: 0, y: t))
            grid.line(to: NSPoint(x: bounds.width, y: t))
        }
        grid.lineWidth = 1
        grid.stroke()
        let diagonal = NSBezierPath()
        diagonal.move(to: view(0, 0))
        diagonal.line(to: view(255, 255))
        diagonal.setLineDash([3, 3], count: 2, phase: 0)
        diagonal.stroke()
        // Other channels first, the edited one on top.
        let order = channels.indices.filter { $0 != editing } + [editing]
        for c in order {
            let color = channels.count == 3 ? Self.channelColors[c] : Theme.ink
            let table = Self.table(channels[c])
            let path = NSBezierPath()
            for x in 0...255 {
                let p = view(x, table[x])
                if x == 0 { path.move(to: p) } else { path.line(to: p) }
            }
            path.lineWidth = c == editing ? 2 : 1
            (c == editing ? color : color.withAlphaComponent(0.5)).setStroke()
            path.stroke()
            guard c == editing else { continue }
            for pt in channels[c] {
                let q = view(pt[0], pt[1])
                let dot = NSBezierPath(rect: NSRect(x: q.x - 3.5, y: q.y - 3.5, width: 7, height: 7))
                Theme.field.setFill()
                dot.fill()
                color.setStroke()
                dot.lineWidth = 1.5
                dot.stroke()
            }
        }
        Theme.panelLine.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()
    }

    /// The 256-entry lookup a natural cubic spline through `points` gives, for
    /// drawing (the engine computes the real table).
    static func table(_ points: [[Int]]) -> [Int] {
        let pts = normalized(points)
        let n = pts.count
        let xs = pts.map { Double($0[0]) }, ys = pts.map { Double($0[1]) }
        guard n >= 2 else { return (0...255).map { $0 } }
        var m = [Double](repeating: 0, count: n)  // second derivatives
        if n > 2 {
            var u = [Double](repeating: 0, count: n)
            for i in 1..<(n - 1) {
                let sig = (xs[i] - xs[i - 1]) / (xs[i + 1] - xs[i - 1])
                let p = sig * m[i - 1] + 2
                m[i] = (sig - 1) / p
                let d = (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]) - (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1])
                u[i] = (6 * d / (xs[i + 1] - xs[i - 1]) - sig * u[i - 1]) / p
            }
            for k in stride(from: n - 2, through: 0, by: -1) { m[k] = m[k] * m[k + 1] + u[k] }
        }
        return (0...255).map { xi in
            let x = Double(xi)
            if x <= xs[0] { return Int(ys[0]) }
            if x >= xs[n - 1] { return Int(ys[n - 1]) }
            var k = 0
            while k < n - 2, x > xs[k + 1] { k += 1 }
            let h = xs[k + 1] - xs[k]
            let a = (xs[k + 1] - x) / h, b = (x - xs[k]) / h
            let y = a * ys[k] + b * ys[k + 1] + ((a * a * a - a) * m[k] + (b * b * b - b) * m[k + 1]) * h * h / 6
            return Int(min(max(y, 0), 255).rounded())
        }
    }
}

// MARK: Levels

/// The Levels editor: input black and white, gamma, and output black and white,
/// for all channels together or for red, green or blue alone.
@MainActor
public final class LevelsView: NSView {
    public struct Values: Equatable, Sendable {
        public var inputBlack: [Int]
        public var inputWhite: [Int]
        public var gamma: [Double]
        public var outputBlack: [Int]
        public var outputWhite: [Int]

        public init(inputBlack: [Int] = [0, 0, 0], inputWhite: [Int] = [255, 255, 255], gamma: [Double] = [1, 1, 1],
                    outputBlack: [Int] = [0, 0, 0], outputWhite: [Int] = [255, 255, 255]) {
            self.inputBlack = inputBlack
            self.inputWhite = inputWhite
            self.gamma = gamma
            self.outputBlack = outputBlack
            self.outputWhite = outputWhite
        }
    }

    public private(set) var values = Values()
    /// The channel buttons: RGB (all three), R, G, B.
    public let channelControl = NSSegmentedControl(labels: ["RGB", "R", "G", "B"], trackingMode: .selectOne,
                                                   target: nil, action: nil)
    public let rows: [NumberRow]
    var onChange: ((Values) -> Void)?
    static let titles = ["Input black", "Input white", "Gamma", "Output black", "Output white"]
    static let rowHeight: CGFloat = 28, gap: CGFloat = 8

    public init(width: CGFloat, labelWidth: CGFloat) {
        rows = Self.titles.map { t in
            t == "Gamma"
                ? NumberRow(min: 0.1, max: 10, step: 0.01, decimals: 2, width: width - labelWidth - 8, label: t)
                : NumberRow(min: 0, max: 255, step: 1, decimals: 0, width: width - labelWidth - 8, label: t)
        }
        let height = (Self.rowHeight + Self.gap) * CGFloat(rows.count + 1) - Self.gap
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: height))
        let channelLabel = Labels.label("Channel:", size: 12, color: Theme.ink)
        channelLabel.alignment = .right
        channelLabel.frame = NSRect(x: 0, y: 6, width: labelWidth, height: 16)
        addSubview(channelLabel)
        channelControl.frame = NSRect(x: labelWidth + 8, y: 2, width: 180, height: 24)
        channelControl.selectedSegment = 0
        channelControl.target = self
        channelControl.action = #selector(channelChanged(_:))
        channelControl.setAccessibilityLabel("Levels channel")
        addSubview(channelControl)
        for (i, row) in rows.enumerated() {
            let y = (Self.rowHeight + Self.gap) * CGFloat(i + 1)
            let l = Labels.label(Self.titles[i] + ":", size: 12, color: Theme.ink)
            l.alignment = .right
            l.frame = NSRect(x: 0, y: y + 6, width: labelWidth, height: 16)
            addSubview(l)
            row.frame.origin = NSPoint(x: labelWidth + 8, y: y)
            row.onChange = { [weak self] v in self?.rowChanged(i, v) }
            addSubview(row)
        }
        show()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    public override var isFlipped: Bool { true }

    /// The channels the controls edit now.
    var selected: [Int] { channelControl.selectedSegment <= 0 ? [0, 1, 2] : [channelControl.selectedSegment - 1] }

    /// Replaces the values without reporting.
    public func set(_ v: Values) {
        values = v
        show()
    }

    public func selectChannel(_ segment: Int) {
        channelControl.selectedSegment = segment
        show()
    }

    func show() {
        let c = selected[0]
        let shown: [Double] = [Double(values.inputBlack[c]), Double(values.inputWhite[c]), values.gamma[c],
                               Double(values.outputBlack[c]), Double(values.outputWhite[c])]
        for (row, v) in zip(rows, shown) { row.show(v) }
    }

    @objc func channelChanged(_ sender: Any?) { show() }

    /// Sets row `i` to `v` on the selected channels. Input black stays below
    /// input white and output black stays at or below output white (the
    /// engine's rule); the other end moves out of the way.
    public func rowChanged(_ i: Int, _ v: Double) {
        var n = values
        for c in selected {
            let b = Int(v.rounded())
            switch i {
            case 0:
                n.inputBlack[c] = min(b, 254)
                n.inputWhite[c] = max(n.inputWhite[c], n.inputBlack[c] + 1)
            case 1:
                n.inputWhite[c] = max(b, 1)
                n.inputBlack[c] = min(n.inputBlack[c], n.inputWhite[c] - 1)
            case 2: n.gamma[c] = min(max((v * 100).rounded() / 100, 0.1), 10)
            case 3:
                n.outputBlack[c] = b
                n.outputWhite[c] = max(n.outputWhite[c], b)
            default:
                n.outputWhite[c] = b
                n.outputBlack[c] = min(n.outputBlack[c], b)
            }
        }
        guard n != values else { return }
        values = n
        show()
        onChange?(n)
    }
}
