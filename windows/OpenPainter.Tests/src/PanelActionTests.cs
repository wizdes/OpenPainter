// W8 done-test 2: every action in the Mac's panels has a Windows test, by name. The list is read
// from the Swift source each run, so a new action on the Mac fails here until a test covers it.
// Stage 2 of W8 adds Tools and Colors to `Sources` and their rows to `Covered`.
using System.Text.RegularExpressions;
using Xunit;

namespace OpenPainter.Tests;

public static partial class PanelActions
{
    /// The Mac's panel sources, and where in each file the panel's own code starts. Before that,
    /// HistoryPanel.swift names commands only to pick their icons.
    public static readonly (string Panel, string File, string From)[] Sources =
    [
        ("History", "mac/OpenPainterKit/Sources/Shell/HistoryPanel.swift", "class HistoryRow"),
        ("Layers", "mac/OpenPainterKit/Sources/Shell/LayersPanel.swift", "class LayerThumbnailCache"),
        ("Layer Properties", "mac/OpenPainterKit/Sources/Shell/LayerProperties.swift", "class LayerPropertiesController"),
    ];

    [GeneratedRegex(@"^\s*//.*$", RegexOptions.Multiline)] private static partial Regex CommentLine();
    [GeneratedRegex(@"""((?:layer|edit|history|preview)\.\w+)""")] private static partial Regex EngineCommand();
    [GeneratedRegex(@"#selector\((\w+)(?:\(_:\))?\)")] private static partial Regex Selector();
    [GeneratedRegex(@"\.on(\w+) = \{")] private static partial Regex RowCallback();
    [GeneratedRegex(@"func (drag\w+)\(")] private static partial Regex Drag();
    [GeneratedRegex(@"public let (\w+) = NS(?:TextField|Slider|PopUpButton|Button)\(")] private static partial Regex Control();
    [GeneratedRegex(@"public private\(set\) var (\w+Button)\b")] private static partial Regex NamedButton();

    /// Each action one Swift source offers, in words that name what it is.
    public static IReadOnlyList<string> Read(string panel, string swift)
    {
        var code = CommentLine().Replace(swift, "");
        var found = new SortedSet<string>(StringComparer.Ordinal);
        void Add(string kind, Regex pattern)
        {
            foreach (Match m in pattern.Matches(code)) found.Add($"{panel}: {kind} {m.Groups[1].Value}");
        }
        Add("command", EngineCommand());
        Add("selector", Selector());
        Add("callback", RowCallback());
        Add("drag", Drag());
        Add("control", Control());
        Add("button", NamedButton());
        return found.ToList();
    }

    public static IReadOnlyList<string> All() => Sources.SelectMany(s =>
    {
        var source = Repo.Read(s.File);
        var from = source.IndexOf(s.From, StringComparison.Ordinal);
        if (from < 0) throw new InvalidDataException($"{s.File} has no {s.From}");
        return Read(s.Panel, source[from..]);
    }).ToList();

    /// Each action, and the Windows tests that prove it against the real engine.
    public static readonly IReadOnlyDictionary<string, string[]> Covered = new Dictionary<string, string[]>
    {
        ["History: command history.jump"] = ["AClickOnARowJumpsToThatStepAndAddsNoStep"],
        ["History: command edit.undo"] = ["UndoAndRedoRunAndFollowThePosition"],
        ["History: command edit.redo"] = ["UndoAndRedoRunAndFollowThePosition"],
        ["History: selector undo"] = ["UndoAndRedoRunAndFollowThePosition"],
        ["History: selector redo"] = ["UndoAndRedoRunAndFollowThePosition"],
        ["History: button undoButton"] = ["UndoAndRedoRunAndFollowThePosition"],
        ["History: button redoButton"] = ["UndoAndRedoRunAndFollowThePosition"],
        ["History: callback Click"] = ["AClickOnARowJumpsToThatStepAndAddsNoStep"],

        ["Layers: command layer.add"] = ["AddPutsANamedLayerAboveTheActiveOne"],
        ["Layers: command layer.delete"] = ["DeleteRemovesTheActiveLayerAndUndoBringsItBack"],
        ["Layers: command layer.duplicate"] = ["DuplicatePutsACopyAboveAndMakesItActive"],
        ["Layers: command layer.merge_down"] = ["MergeDownJoinsTheLayerIntoTheOneBelow"],
        ["Layers: command layer.move_up"] = ["MoveUpAndMoveDownSwapTheActiveLayer"],
        ["Layers: command layer.move_down"] = ["MoveUpAndMoveDownSwapTheActiveLayer"],
        ["Layers: command layer.move"] = ["DraggingARowMovesTheLayerAndShowsTheLine"],
        ["Layers: command layer.properties"] = ["ThePropertiesButtonOpensItForTheActiveLayer"],
        ["Layers: command layer.set_active"] = ["AClickOnARowSelectsItAndAddsNoStep"],
        ["Layers: command layer.set_visible"] = ["TheEyeHidesAndShowsALayerAndTheDetailLineSaysSo"],
        ["Layers: command layer.thumbnail"] = ["AThumbnailIsTheEnginesPictureOfThatLayerAndFollowsItsPixels", "AHistoryJumpReusesAPictureItMadeBefore"],
        ["Layers: selector action"] = ["TheButtonsAreTheMacsInItsOrderAndFollowTheEngine"],
        ["Layers: selector eyeClicked"] = ["TheEyeHidesAndShowsALayerAndTheDetailLineSaysSo"],
        ["Layers: callback Click"] = ["AClickOnARowSelectsItAndAddsNoStep"],
        ["Layers: callback ToggleVisible"] = ["TheEyeHidesAndShowsALayerAndTheDetailLineSaysSo"],
        ["Layers: callback OpenProperties"] = ["ADoubleClickOnARowSelectsItAndOpensItsProperties"],
        ["Layers: callback Ready"] = ["AThumbnailIsTheEnginesPictureOfThatLayerAndFollowsItsPixels"],
        ["Layers: drag dragMoved"] = ["DraggingARowMovesTheLayerAndShowsTheLine"],
        ["Layers: drag dragEnded"] = ["DraggingARowMovesTheLayerAndShowsTheLine", "AListThatChangesDuringADragWaitsForTheDrop"],

        ["Layer Properties: command layer.properties"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: command layer.set_visible"] = ["TheEyeHidesAndShowsALayerAndTheDetailLineSaysSo"],
        ["Layer Properties: command preview.begin"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: command preview.update"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: command preview.commit"] = ["OkAfterManyChangesIsOneStepWithTheLastValues"],
        ["Layer Properties: command preview.cancel"] = ["CancelLeavesLayersHistoryAndPixelsAsTheyWere", "OkWithNothingChangedOrChangedBackAddsNoStep"],
        ["Layer Properties: control nameField"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: control visibleBox"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: control blendPopup"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: control opacitySlider"] = ["TheTypedOpacityAndTheSliderFollowEachOther"],
        ["Layer Properties: control opacityField"] = ["TheTypedOpacityAndTheSliderFollowEachOther"],
        ["Layer Properties: button okButton"] = ["OkAfterManyChangesIsOneStepWithTheLastValues", "EnterIsOk"],
        ["Layer Properties: button cancelButton"] = ["CancelLeavesLayersHistoryAndPixelsAsTheyWere", "TheCloseButtonAndEscapeAreCancel"],
        ["Layer Properties: selector changed"] = ["EveryControlChangesThePreviewAndTheDocumentStaysAsItWas"],
        ["Layer Properties: selector sliderMoved"] = ["TheTypedOpacityAndTheSliderFollowEachOther"],
        ["Layer Properties: selector opacityTyped"] = ["TheTypedOpacityAndTheSliderFollowEachOther"],
        ["Layer Properties: selector cancel"] = ["CancelLeavesLayersHistoryAndPixelsAsTheyWere"],
        ["Layer Properties: selector ok"] = ["OkAfterManyChangesIsOneStepWithTheLastValues"],
        ["Layer Properties: callback Close"] = ["ThePanelCommandsAreOffWhileItIsUpAndComeBack"],
    };

    /// What is wrong with a list of actions against the covered table, each by name.
    public static IReadOnlyList<string> Problems(IReadOnlyList<string> actions, IReadOnlyDictionary<string, string[]> covered, IReadOnlySet<string> testNames)
    {
        var problems = new List<string>();
        foreach (var action in actions)
            if (!covered.ContainsKey(action)) problems.Add($"no Windows test: {action}");
        foreach (var (action, tests) in covered)
        {
            if (!actions.Contains(action)) problems.Add($"no such action on the Mac: {action}");
            foreach (var test in tests)
                if (!testNames.Contains(test)) problems.Add($"{action}: no test named {test}");
        }
        return problems;
    }

    [GeneratedRegex(@"\[(?:AvaloniaFact|Fact|Theory)\][^\n]*\n(?:\s*\[[^\n]*\n)*\s*public (?:static )?void (\w+)\(")] private static partial Regex TestMethod();

    /// Every test method in the test folder, by name. A renamed test fails here.
    public static IReadOnlySet<string> TestNames() =>
        Directory.GetFiles(Repo.File("windows/OpenPainter.Tests/src"), "*.cs").SelectMany(file => TestMethod().Matches(File.ReadAllText(file).Replace("\r\n", "\n")).Select(m => m.Groups[1].Value)).ToHashSet();
}

public sealed class PanelActionTests
{
    [Fact]
    public void EveryMacPanelActionHasAWindowsTest()
    {
        var problems = PanelActions.Problems(PanelActions.All(), PanelActions.Covered, PanelActions.TestNames());
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [Fact]
    public void TheListIsReadFromTheSwiftSourceAndHoldsEveryKind()
    {
        var all = PanelActions.All();
        Assert.Contains("History: command history.jump", all);
        Assert.Contains("Layers: command layer.merge_down", all);
        Assert.Contains("Layers: callback OpenProperties", all);
        Assert.Contains("Layers: drag dragEnded", all);
        Assert.Contains("Layer Properties: control opacityField", all);
        Assert.Contains("Layer Properties: command preview.commit", all);
        Assert.True(all.Count >= 40, $"only {all.Count} actions");
    }

    [Fact]
    public void AMacActionWithNoWindowsTestFailsByName()
    {
        var read = PanelActions.Read("Layers", "e.run(\"layer.new_thing\")\n// e.run(\"layer.in_a_comment\")\n");
        Assert.Equal(["Layers: command layer.new_thing"], read);
        var problems = PanelActions.Problems(read, new Dictionary<string, string[]>(), new HashSet<string>());
        Assert.Equal(["no Windows test: Layers: command layer.new_thing"], problems);
    }

    [Fact]
    public void ACoveredRowWithNoSuchActionOrTestFailsByName()
    {
        var covered = new Dictionary<string, string[]> { ["a"] = ["GoneTest"], ["b"] = ["GoneTest"] };
        var problems = PanelActions.Problems(["a"], covered, new HashSet<string>());
        Assert.Contains("a: no test named GoneTest", problems);
        Assert.Contains("no such action on the Mac: b", problems);
    }

    [Fact]
    public void ThePanelsLeftForStage2HaveAPlace()
    {
        // When Tools and Colors join, this list grows, and their actions fail until covered.
        Assert.Equal(["History", "Layers", "Layer Properties"], PanelActions.Sources.Select(s => s.Panel));
        Assert.All(PanelActions.Sources, s => Assert.True(File.Exists(Repo.File(s.File)), s.File));
    }
}
