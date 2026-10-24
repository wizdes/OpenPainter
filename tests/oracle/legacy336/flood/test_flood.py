import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[4]
FLOOD = ROOT / 'tests/oracle/legacy336/flood'


class FloodOracleTests(unittest.TestCase):
    def test_all_42_masks_match_the_manifest(self):
        entries = json.loads((FLOOD / 'manifest.json').read_text())
        self.assertEqual(len(entries), 42)
        self.assertEqual({e['status'] for e in entries}, {'ok'})
        for entry in entries:
            source = ROOT / entry['input']
            output = ROOT / entry['output']
            self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), entry['input_sha256'])
            self.assertEqual(hashlib.sha256(output.read_bytes()).hexdigest(), entry['output_sha256'])
            p = entry['parameters']
            self.assertEqual(len(output.read_bytes()), p['width'] * p['height'])
            self.assertLessEqual(set(output.read_bytes()), {0, 255})

    def test_extracted_legacy_methods_have_no_edits(self):
        source = (ROOT / '.pdn-src/src/tools/FloodToolBase.cs').read_text()
        start = source.index('        private static bool CheckColor(')
        end = source.index('        protected abstract void OnFillRegionComputed', start)
        generated = (ROOT / 'tools/pdn-oracle/Generated/FloodExtract.cs').read_text()
        self.assertIn(source[start:end], generated)


if __name__ == '__main__':
    unittest.main()
