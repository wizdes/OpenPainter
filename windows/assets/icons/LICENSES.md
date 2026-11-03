# Icon licenses

`docs/windows/icons.toml` says where each file here came from.

## Fluent System Icons

- Package: `@fluentui/svg-icons`, version 1.1.345, from npm
- Project: https://github.com/microsoft/fluentui-system-icons
- License: MIT

The package states its license as MIT in `package.json` but holds no license file. The text below is the `LICENSE` file from the project's repository, at commit 06f559c29c761ca979894d07a14b3a44c94a24d1, the last commit that changed that file.

The Fluent files here are unchanged from the package, apart from their file names. `fluent-used.txt` lists each one's package name and SHA-256, `docs/windows/icons.toml` records the hash for every use, and `tools/ui-look/tests` checks both, so replaced art fails a test. The folders `16` and `12` hold the smaller art of a few of these icons, also unchanged. `small-art.txt` lists each one, and `windows/OpenPainter.Tests` checks them.

This notice must ship with the app. W19 adds it to the app's third-party notices.

```
MIT License

Copyright (c) 2020 Microsoft Corporation

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Own work

Rows with `source = "own-work"` were drawn for this project. They use the project's own MIT license, in `LICENSE` at the top of this repository. None is copied or traced from SF Symbols or paint.net.
