# W8: Panels and shortcuts

This file holds the results of W8, one section per done-test item. Each item says how to run it. Root fills in the results of the last runs. A result in this file comes from a real run, or says NOT RUN.

Set these first in a Git Bash window:

```
export DOTNET_ROOT="$USERPROFILE/.dotnet10"
export PATH="$USERPROFILE/.dotnet10:$PATH:/c/Program Files (x86)/Microsoft Visual Studio/Installer"
```

Build the app for real windows, then run the harness:

```
dotnet publish windows/OpenPainter/OpenPainter.csproj -c Release -r win-x64 -o windows/OpenPainter/bin/publish
dotnet build windows/harness/OpenPainter.Harness.csproj -c Release
windows/harness/bin/Release/net10.0-windows/win-x64/OpenPainter.Harness.exe --app windows/OpenPainter/bin/publish/OpenPainter.exe --out windows/harness/bin/last-run --parts <parts> --ignore-disk
```

## 1. Every panel action is tested headless, against the real engine

Run: `dotnet test windows/OpenPainter.Tests --filter "FullyQualifiedName~PanelTests|FullyQualifiedName~ColorsPanelTests|FullyQualifiedName~OptionBindingTests"`.

The stage 3 tests add the same actions through the menus and keys: `FocusRuleTests`.

Result: root fills this in.

## 2. A test lists every panel action from the Mac's panel sources

Run: `dotnet test windows/OpenPainter.Tests --filter "FullyQualifiedName~PanelAction"`.

An action on the Mac with no Windows test fails by name.

Result: root fills this in.

## 3. Shortcuts match the Mac's with Ctrl for Command

Run: `dotnet test windows/OpenPainter.Tests --filter "FullyQualifiedName~ShortcutTableTests"`.

The table is `docs/windows/shortcuts-windows.md`. A difference that is not in the table fails by name, and so does a row that no longer differs. The test also holds the list of keys that later milestones turn on.

Result: 0 unexplained differences at stage 3. Root fills in the last run.

## 4. Focus rules follow `Shell/FocusRouter.swift`, each rule with a test

Run: `dotnet test windows/OpenPainter.Tests --filter "FullyQualifiedName~FocusRuleTests"`.

| Rule | Test |
| --- | --- |
| The canvas context sends commands to the document | `TheCanvasContextSendsCommandsToTheDocument` |
| A control that is not text still sends them | `ANonTextControlStillSendsCommandsToTheDocument` |
| A text field keeps its commands and letters | `ATextFieldKeepsItsCommandsAndLettersAndTheDocumentGetsNothing`, `TheFieldKeepsThisKey` |
| Other menu keys pass a field | `OtherMenuKeysPassAField`, `AMenuKeyStillWorksWhileAFieldHasFocus` |
| A dialog gives the document nothing | `ADialogGivesTheDocumentNothingFromAnyWindow` |
| A panel with no field forwards commands | `APanelWithNoFieldForwardsCommandsAndCanvasKeys` |
| The Colors hex field takes its keys while it edits | `TheColorsHexFieldTakesItsKeysWhileItEdits` |
| After Enter in a color field, keys go to the canvas | `AfterEnterInTheHexFieldKeysGoToTheCanvas`, `AfterEnterInAChannelFieldKeysGoToTheCanvas` |
| Tab leaves the canvas, and Ctrl+Tab stays | `TabLeavesTheCanvasAndCtrlTabStays`, and the committed `LeavingTheNativeCanvasGoesWhereTabGoes` |

Result: root fills this in.

## 5. Color math passes the Swift vectors

Run: `dotnet test windows/OpenPainter.Tests --filter "FullyQualifiedName~VectorTests"`.

Result: root fills this in.

## 6. Root looks at one real-window shot per panel, light and dark

Run the harness with `--parts panels`. The shots land in `windows/harness/bin/last-run/shots`.

Result: root fills this in.

## 7. `launch paint` passes 50 of 50 `pixels` runs

Run the harness with `--parts pixels --first-frame-runs 50`.

Result: root fills this in.

## 8. The levels

Run: `cargo xtask check --tier fast`, then `--tier milestone`, then `--tier window` when the PC is quiet.

`milestone` prints one row per critical flow from `docs/windows/flows.md`. The flows that W9, W10 and W11 make possible print NOT RUN by name, so the level fails until then.

`window` now has the `alt-states` part. Its row for the system's own high-contrast switch prints NOT RUN, because that setting belongs to the user, so the level fails on it until the user runs it or waives it.

Result: root fills this in.

## 9. A fresh review

Result: root fills this in.

## Hover, press, focus and high contrast

Run: `dotnet test windows/OpenPainter.Tests --filter "FullyQualifiedName~ControlStateTests"`, and the harness with `--parts alt-states`.

- The tests move a headless pointer over each control type, press it and give it keyboard focus, in light, dark, Aquatic and Desert. Each state must change pixels.
- The contrast test walks the main window, the four panels, Layer Properties and two open menus in light, dark and the four Windows contrast themes. All text must reach 4.5 to 1.
- The harness does the same steps in a real window by posting messages to the app's own windows. The user's pointer stays where it is. A test hook holds back the leave message that Windows sends when the real pointer is elsewhere, so this is not a real hover. A real pointer stays owed from W7.
- High contrast in the harness uses the colors from Windows' own `.theme` files. The system's switch stays off.
