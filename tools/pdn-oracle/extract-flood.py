"""Copy the unmodified OpenPDN 3.36 flood detection methods into the oracle."""
from pathlib import Path
import os

root = Path(__file__).resolve().parents[2]
source_root = Path(os.environ.get('OpenPdnSourceRoot', str(root / '.pdn-src')))
source = (source_root / 'src/tools/FloodToolBase.cs').read_text()
start = source.index('        private static bool CheckColor(')
end = source.index('        protected abstract void OnFillRegionComputed', start)
body = source[start:end]
out = root / 'tools/pdn-oracle/Generated/FloodExtract.cs'
out.parent.mkdir(exist_ok=True)
out.write_text('''// Extracted without editing the methods from OpenPDN 3.36, commit cca476b (MIT).
// Copyright dotPDN LLC, Rick Brewster, Tom Jackson, and contributors.
using System;
using System.Collections.Generic;
using System.Drawing;
namespace PaintDotNet.Tools {
internal static class FloodExtract {
''' + body + '\n}\n}\n')
