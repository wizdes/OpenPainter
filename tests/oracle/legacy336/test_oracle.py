import hashlib
import json
import pathlib
import struct
import unittest
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
ORACLE = ROOT / 'tests/oracle/legacy336'


def rgba_at(path, x, y):
    data = path.read_bytes()
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    pos = 8
    compressed = bytearray()
    while pos < len(data):
        size = int.from_bytes(data[pos:pos + 4], 'big')
        kind = data[pos + 4:pos + 8]
        chunk = data[pos + 8:pos + 8 + size]
        pos += size + 12
        if kind == b'IHDR':
            width, height = struct.unpack('>II', chunk[:8])
        elif kind == b'IDAT':
            compressed.extend(chunk)
        elif kind == b'IEND':
            break
    assert 0 <= x < width and 0 <= y < height
    raw = zlib.decompress(compressed)
    row_start = y * (1 + width * 4)
    assert raw[row_start] == 0
    start = row_start + 1 + x * 4
    return tuple(raw[start:start + 4])


class OracleOutputTests(unittest.TestCase):
    def test_generated_output_stays_below_40_mb(self):
        files = list((ROOT / 'tests/fixtures').rglob('*')) + list(ORACLE.rglob('*'))
        size = sum(path.stat().st_size for path in files if path.is_file())
        self.assertLess(size, 40_000_000)

    def test_effects_all_manifest_keeps_every_effect(self):
        entries = json.loads((ORACLE / 'effects/manifest.json').read_text())
        names = {entry['op'] for entry in entries}
        self.assertGreaterEqual(len(names), 13)
        core = {'Invert Colors', 'Desaturate', 'Sepia', 'Brightness and Contrast',
                'Hue and Saturation', 'Posterize', 'Auto-Level', 'Levels', 'Curves',
                'Gaussian Blur', 'Add Noise', 'Frosted Glass', 'Clouds'}
        entries = [entry for entry in entries if entry['op'] in core
                   and entry['parameters'] == {}]
        self.assertEqual(len(entries), 13 * 4)

    def test_vectors_cover_all_byte_pairs(self):
        data = json.loads((ORACLE / 'vectors/fast_scale_byte_by_byte.json').read_text())
        self.assertEqual(len(data['values']), 65536)
        self.assertEqual(data['values'][0], 0)
        self.assertEqual(data['values'][-1], 255)

    def test_lerp_and_blend_vectors_cover_pairs(self):
        directory = ORACLE / 'vectors'
        for name in ('color_bgra_lerp', 'color_bgra_blend'):
            data = json.loads((directory / f'{name}.json').read_text())
            self.assertEqual(len(data['values']), 5)
            self.assertTrue(all(len(values) == 65536 for values in data['values']))

    def test_gaussian_uses_tent_row(self):
        data = json.loads((ORACLE / 'vectors/gaussian_row.json').read_text())
        self.assertEqual(data['radius'], 2)
        self.assertEqual(data['weights'], [16, 32, 48, 32, 16])

    def test_gaussian_impulse_and_hidden_color(self):
        gaussian = ORACLE / 'effects/Gaussian_Blur_default_impulse_64.png'
        self.assertEqual(rgba_at(gaussian, 32, 32), (28, 28, 28, 255))
        alpha_edge = ROOT / 'tests/fixtures/alpha_edge_64.png'
        self.assertEqual(rgba_at(alpha_edge, 0, 0), (255, 26, 13, 0))

    def test_core_adjustments_render_from_openpdn_ops(self):
        entries = json.loads((ORACLE / 'effects/manifest.json').read_text())
        for name in ('Hue and Saturation', 'Auto-Level', 'Levels', 'Curves'):
            rendered = [entry for entry in entries if entry['op'] == name
                        and entry['status'] == 'ok']
            self.assertGreaterEqual(len(rendered), 4, name)

    def test_effect_classes_render(self):
        entries = json.loads((ORACLE / 'effects/manifest.json').read_text())
        for name in ('Brightness and Contrast', 'Posterize', 'Gaussian Blur'):
            rendered = [entry for entry in entries if entry['op'] == name
                        and entry['status'] == 'ok']
            self.assertGreaterEqual(len(rendered), 4, name)

    def test_seeded_effects_have_fixed_output(self):
        entries = json.loads((ORACLE / 'effects/manifest.json').read_text())
        for name in ('Add Noise', 'Frosted Glass', 'Clouds'):
            rendered = [entry for entry in entries if entry['op'] == name
                        and entry['status'] == 'seed-patched']
            self.assertGreaterEqual(len(rendered), 4, name)

    def test_every_effect_png_has_a_manifest_entry(self):
        directory = ORACLE / 'effects'
        entries = json.loads((directory / 'manifest.json').read_text())
        actual = {path.name for path in directory.glob('*.png')}
        listed = {pathlib.Path(entry['output']).name for entry in entries
                  if entry['output']}
        self.assertEqual(actual, listed)

    def test_manifest_hashes_match_files(self):
        for directory in (ROOT / 'tests/fixtures', ORACLE / 'vectors',
                          ORACLE / 'blend', ORACLE / 'resample', ORACLE / 'effects'):
            for entry in json.loads((directory / 'manifest.json').read_text()):
                self.assertEqual(entry['commit'],
                                 'cca476b0df2a2f70996e6b9486ec45327631568c')
                for key in ('input', 'output'):
                    if entry[key]:
                        path = ROOT / entry[key]
                        digest = hashlib.sha256(path.read_bytes()).hexdigest()
                        self.assertEqual(digest, entry[f'{key}_sha256'])

    def test_compile_list_has_no_restricted_sources(self):
        project = (ROOT / 'tools/pdn-oracle/PdnOracle.csproj').read_text()
        for forbidden in ('.resx', '.resources', '.png', 'GpcWrapper', 'GPC'):
            self.assertNotIn(forbidden, project)
        self.assertNotIn('**/*.cs', project)
        self.assertNotIn('System.Drawing.Common', project)


if __name__ == '__main__':
    unittest.main()
