# Remove Background test photos (M53)

Ten named photos with a known foreground mask, for `mac/Tests/OpenPainterKitTests/RemoveBackgroundTests.swift`. The paint.net corpus planned in M02 is not available, so each photo is a composite: one or two cutouts (PNG with transparency) on a generated background. The true mask is the alpha the cutouts were blended with, so it is exact.

Each photo has two files:

- `<name>.jpg`: the composite (JPEG, quality 0.92, sRGB).
- `<name>-mask.png`: the true mask, 8-bit gray, one byte per pixel. The tests threshold it at 128.

Total size: 1.4 MB.

## Rebuild

```
swift tests/fixtures/remove-bg/make-fixtures.swift <cache-dir>
```

`make-fixtures.swift` downloads each cutout from Wikimedia Commons into `<cache-dir>`, checks its SHA-256 against `sources.json`, draws the background from a fixed seed and writes both files. A rebuild on macOS 27 gives the same bytes. `sources.json` holds every URL, hash, license, author, size, background and placement.

## The photos

| Name | Kind | Size | Background (generated) | Cutout |
| --- | --- | --- | --- | --- |
| 01-person | person | 480 × 640 | sky and field | navy_uniform |
| 02-pet | pet | 480 × 640 | wood grain | jack_russell |
| 03-product | product | 640 × 480 | dark slate | cassette |
| 04-plant | plant | 560 × 600 | tiled wall | ficus |
| 05-hair | hair-like edges (fur) | 480 × 640 | bokeh circles | cat_stretch |
| 06-glass | glass | 480 × 640 | studio gradient | agua_glass |
| 07-car | car | 640 × 420 | sky and road | race_car |
| 08-sign | text on a sign | 640 × 400 | brick wall | amos_owen_sign |
| 09-two-objects | two objects | 640 × 440 | green cloth | strawberry, cup_of_tea |
| 10-low-contrast | low-contrast object | 560 × 560 | cream | white_flower |

## Sources and licenses

All cutouts come from Wikimedia Commons. Each is CC0 or in the public domain. The URL used is the Commons thumbnail (960 px wide) or the original file, as listed in `sources.json`.

| Key | Commons file | License | Author |
| --- | --- | --- | --- |
| navy_uniform | [160803-N-RY232-001 - Navy Working Uniform (NWU) Type III.png](https://commons.wikimedia.org/wiki/File:160803-N-RY232-001_-_Navy_Working_Uniform_(NWU)_Type_III.png) | Public domain (U.S. Navy work) | U.S. Navy |
| jack_russell | [Transparent-jack-russell-puppy-dog.png](https://commons.wikimedia.org/wiki/File:Transparent-jack-russell-puppy-dog.png) | CC0 | PxHere.com |
| cassette | [Sony Duad (Japanese market Ferri Chrome) cassette (transparent).png](https://commons.wikimedia.org/wiki/File:Sony_Duad_(Japanese_market_Ferri_Chrome)_cassette_(transparent).png) | CC0 | Retired electrician |
| ficus | [Ficus lyrata indoor house second floor Transparent - July 2026.png](https://commons.wikimedia.org/wiki/File:Ficus_lyrata_indoor_house_second_floor_Transparent_-_July_2026.png) | CC0 | Ranch9613 |
| cat_stretch | [Cat Stretch.png](https://commons.wikimedia.org/wiki/File:Cat_Stretch.png) | CC0 | Lisafern |
| agua_glass | [Agua de Valencia's glass.png](https://commons.wikimedia.org/wiki/File:Agua_de_Valencia%27s_glass.png) | CC0 | Gerhard Gellinger |
| race_car | [GregBiffleCar-transparent.png](https://commons.wikimedia.org/wiki/File:GregBiffleCar-transparent.png) | Public domain (U.S. Army work) | National Guard Bureau |
| amos_owen_sign | [Amos Owen Lane road signs at Land of Memories Park Mankato isolated.png](https://commons.wikimedia.org/wiki/File:Amos_Owen_Lane_road_signs_at_Land_of_Memories_Park_Mankato_isolated.png) | CC0 | Pingnova |
| strawberry | [Strawberry (transparent background).png](https://commons.wikimedia.org/wiki/File:Strawberry_(transparent_background).png) | CC0 | Paolo Neo |
| cup_of_tea | [Cup of tea.png](https://commons.wikimedia.org/wiki/File:Cup_of_tea.png) | CC0 | Ian Pegg |
| white_flower | [Cactus flower extracted.png](https://commons.wikimedia.org/wiki/File:Cactus_flower_extracted.png) | CC0 | Andrikkos |

## Replaced candidate

The first car was a CC0 colour illustration ([1979 Pontiac Firebird Esprit illustration, colour on transparent.png](https://commons.wikimedia.org/wiki/File:1979_Pontiac_Firebird_Esprit_illustration,_colour_on_transparent.png)). Its alpha includes loose pencil strokes around the car, which Vision leaves out, so it reached IoU 0.90 at one size and 0.896 at another. It was replaced with the race-car photo above (IoU 0.94). The bar stayed at 0.9.
