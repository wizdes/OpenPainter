# OpenPDN 3.36 pixel oracle

Run from the repository root with .NET 8:

```sh
bash tools/pdn-oracle/fetch-source.sh
bash tools/pdn-oracle/run-all.sh
```

`fetch-source.sh` clones rivy/OpenPDN into `.pdn-src`, checks out the pinned commit, and rejects a different commit. The project compiles its listed files by path. Source files and Windows resources are never copied into the repository. `ALLOWLIST.md` names the compiled files and platform stubs.

`run-all.sh` builds the CLI, then runs `fixtures`, `vectors`, `blend`, `resample`, `effects-all`, and `flood`. The CLI can also run one effect with `dotnet run --project tools/pdn-oracle -- effect 'Invert Colors' '{}'`. Each output folder gets `manifest.json` with input and output hashes.

- `fixtures`: seven deterministic RGBA PNGs in `tests/fixtures/`.
- `vectors`: all 65,536 `Utility.FastScaleByteByByte` byte pairs, `ColorBgra.Lerp` and `ColorBgra.Blend` pair grids at five fixed weights, and the Gaussian radius-2 tent row.
- `blend`: 14 original blend modes at five opacity values on two fixture pairs.
- `resample`: four original Surface resamplers at four sizes.
- `effects-all`: 10 required effects on four fixtures. Brightness and Contrast, Posterize, and Gaussian Blur run their original effect classes. Add Noise, Frosted Glass, and Clouds run source shims with fixed seeds and have `seed-patched` status. Other concrete effect classes are recorded as `not-run` because this harness has no compile entry or CLI dispatcher for them. The manifest gives the reason for each one.
- `flood`: 42 binary masks for five fixtures at 0, 25, 50, and 100 percent in contiguous and global modes, plus two threshold cases. The build copies the color check and region search methods unchanged from `FloodToolBase.cs`. UI and selection clipping are stubbed. The output bytes are 0 or 255 in row order. The source scans shared edges only, so diagonal pixels do not connect.

Clouds uses opaque black as the primary color and opaque white as the secondary color. Its fixed instance seed is `0x5a`. Add Noise and Frosted Glass reset their random generator to `0x51ed270b` before each render.

To add an operation, add its OpenPDN files to the explicit list in `PdnOracle.csproj`, update `ALLOWLIST.md`, add its CLI dispatch and parameter cases in `Program.cs`, then add a regression in `tests/oracle/legacy336/test_oracle.py`. Generate the outputs twice and compare hashes before accepting them.

For source archives in an environment where Git cannot create `.pdn-src/.git`, set `PDN_ORACLE_SKIP_FETCH=1` only after placing the exact pinned source tree in `.pdn-src`. Normal runs always fetch and verify Git HEAD.
