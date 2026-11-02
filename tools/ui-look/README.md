# ui-look

This tool draws fixed pictures of the OpenPainter window for Windows and checks the icon records. It is W6's work. It has no engine and no working tools, and it opens no window on the screen.

## Draw the pictures

```
dotnet run --project tools/ui-look/app -- --out <folder>
```

This writes 12 PNG files, named `<scene>-<light|dark>.png`. The scenes are `main`, `text`, `selection`, `curves`, `hue-saturation` and `colors-expanded`. The first five are 2752 by 1856 pixels. `colors-expanded` is 616 by 1068 pixels, the same as the Mac's panel shot.

To see every icon on one sheet:

```
dotnet run --project tools/ui-look/app -- --icon-sheet <file.png>
```

## Run the checks

```
dotnet test tools/ui-look/tests
```

Both commands need .NET 10. Set `DOTNET_ROOT` and `PATH` to it first.

## What is where

| Path | What it holds |
| --- | --- |
| `app/src/Tokens.cs` | Colors, sizes, font sizes and spacing. W7 reuses this file. Light and dark differ only here. |
| `app/src/Scenes.cs` | What each scene shows. |
| `app/src/Shell.cs`, `Panels.cs`, `Dialogs.cs` | The window, the four panels and the two dialogs. |
| `app/src/Icons.cs` | Reads an icon's SVG file and draws it in one color. |
| `app/src/Renderer.cs` | Draws each scene to a PNG. |
| `tests/src/MacSymbols.cs` | Finds the SF Symbol names in the Mac source. |
| `../../windows/assets/icons/` | The icon files and their licenses. |
| `../../docs/windows/icons.toml` | One row per Mac symbol. |
| `../../docs/windows/w6-look.md` | Each Windows change and why, and where the picture still differs from the Mac. |

## One trap

When Avalonia 12.1.4 draws to a bitmap, a `Border` that has both a `BoxShadow` and a child draws the child at the wrong scale. Keep the shadow on its own `Border` next to the content. `Panels.Frame` shows how.
