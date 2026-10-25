// The app-wide state of the main window: the open images in strip order, the
// active one, the shared tool state, the floating panels and the effect
// catalog. Engine events refresh the editor they belong to.
import AppKit

@MainActor
public final class Workspace {
    /// The app's workspace (set by the app at launch).
    public static var shared: Workspace?

    public let engine: Engine
    public let tools: ToolState
    public let panels: PanelManager
    public private(set) var editors: [Editor] = []
    public private(set) weak var active: Editor?
    /// `effects.list`, read once.
    public let effects: [Engine.EffectInfo]
    public let toolsView: ToolsPanelView
    public let colorsView: ColorsPanelView
    public private(set) var layersView: LayersPanelView!
    public private(set) var historyView: HistoryPanelView!
    /// Target of the Window menu's panel items and the toolbar's panel toggles.
    public private(set) lazy var menuTarget = PanelMenuTarget(panels: panels)
    /// Opens one file as a document (M17 drag and drop). The app sets it.
    public var fileOpener: FileOpener?
    /// Appearance, units, rulers and pixel grid (M56), in the panel store's defaults.
    public let settings: AppSettings
    private var observers: [(AnyObject?, () -> Void)] = []
    private var eventTask: Task<Void, Never>?

    public init(engine: Engine, tools: ToolState? = nil, store: PanelStore = PanelStore(),
                hidesPanelsOnDeactivate: Bool = true) {
        self.engine = engine
        let tools = tools ?? ToolState()
        self.tools = tools
        panels = PanelManager(store: store, hidesOnDeactivate: hidesPanelsOnDeactivate)
        settings = AppSettings(defaults: store.defaults)
        do {
            effects = try engine.effects()
        } catch {
            FileHandle.standardError.write(Data("OpenPainter: effects.list failed: \(error)\n".utf8))
            effects = []
        }
        toolsView = ToolsPanelView(tools: tools)
        colorsView = ColorsPanelView(tools: tools)
        layersView = LayersPanelView(workspace: self)
        historyView = HistoryPanelView(workspace: self)
        panels.panels[.tools]!.contentView = toolsView
        panels.panels[.colors]!.contentView = colorsView
        panels.panels[.layers]!.contentView = layersView
        panels.panels[.history]!.contentView = historyView
        for p in panels.panels.values { p.router = { [weak self] in self?.active?.windowController?.window } }
        panels.stage = { [weak self] in self?.active?.windowController?.stageInScreen }
        panels.onChange = { [weak self] in self?.notify() }
        wireColors()
        settings.applyAppearance()
        settings.observe(self) { [weak self] in self?.settings.applyAppearance() }
    }

    /// Reads engine events and refreshes the editor they name. Call once.
    public func startEvents() {
        guard eventTask == nil else { return }
        let stream = engine.events
        eventTask = Task { @MainActor [weak self] in
            for await event in stream {
                guard let self else { return }
                switch event.kind {
                case .docChanged, .historyChanged, .layersChanged:
                    self.editors.first { $0.document.id == event.doc }?.scheduleRefresh()
                case .progress: EffectDialogController.progressEvent(event)
                case .recovery:
                    // M57: a failed recovery write shows in the status bar and a sheet.
                    let host = self.editors.first { $0.document.id == event.doc }?.host as? ImageDocument
                    host?.recoveryEvent(event.json)
                default: break
                }
            }
        }
    }

    public func stopEvents() {
        eventTask?.cancel()
        eventTask = nil
    }

    public func observe(_ owner: AnyObject, _ body: @escaping () -> Void) {
        observers.append((owner, body))
    }

    func notify() {
        observers.removeAll { $0.0 == nil }
        for (_, body) in observers { body() }
        layersView.reload()
        historyView.reload()
    }

    // MARK: Editors

    public func add(_ editor: Editor) {
        editors.append(editor)
        editor.observe(self) { [weak self, weak editor] in
            guard let self, let editor else { return }
            if editor === self.active {
                self.layersView.reload()
                self.historyView.reload()
            }
            for o in self.observers { o.1() }
        }
        if active == nil { active = editor }
        notify()
    }

    public func remove(_ editor: Editor) {
        editors.removeAll { $0 === editor }
        if active === editor { active = editors.last }
        if editors.isEmpty { panels.hideAll() }
        notify()
    }

    /// Makes an editor active and brings its window to the front.
    public func activate(_ editor: Editor) {
        setActive(editor)
        editor.windowController?.window?.makeKeyAndOrderFront(nil)
    }

    /// Records the active editor (its window became main).
    public func setActive(_ editor: Editor) {
        guard active !== editor else { return }
        active = editor
        notify()
    }

    /// Image strip drag: moves an editor to a new position.
    public func move(_ editor: Editor, to index: Int) {
        guard let from = editors.firstIndex(where: { $0 === editor }) else { return }
        editors.remove(at: from)
        editors.insert(editor, at: min(max(index, 0), editors.count))
        notify()
    }
}

extension Workspace {
    /// Colors panel hooks (M36): More and Less resize the panel; palette
    /// reports go to the active image's status bar.
    fileprivate func wireColors() {
        var toolsBeforeExpand: CGRect?
        colorsView.onExpandedChange = { [weak self] on in
            guard let self else { return }
            if on {
                toolsBeforeExpand = self.panels.setColorsExpanded(true)
            } else {
                self.panels.setColorsExpanded(false, restoreTools: toolsBeforeExpand)
                toolsBeforeExpand = nil
            }
        }
        colorsView.onReport = { [weak self] text in self?.active?.report(text) }
    }
}
