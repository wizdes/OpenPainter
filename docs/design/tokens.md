# OpenPainter draft tokens

These are shared starting values for macOS 15 and later. The chosen window design can adjust them after visual and keyboard review.

| Token | Value | Use |
| --- | --- | --- |
| Space 1–6 | 4, 6, 8, 12, 16, 24 px | Gaps and insets |
| Control height | 27 px | Small fields and buttons |
| Radius 1–3 | 4, 6, 10 px | Fields, image tabs, floating panels |
| Type small | 10–11 px | Secondary data in a dense panel |
| Type body | 12–13 px | Controls and row labels |
| Type title | 13–14 px, weight 600 | Window and panel titles |
| Panel width | Tools 190–198 px; Colors 197–244 px; Layers and History 226–258 px | Compact widths at 1440 px |
| Minimum hit area | 28 × 28 px in this mockup | Tool cells; increase for the real app where needed |

| Role | Light | Dark |
| --- | --- | --- |
| Window chrome | `#f5f5f6` | `#353a42` |
| Panel | `#f9f9fa` | `#363b43` |
| Field | `#ffffff` | `#262b32` |
| Text | `#202329` | `#eef0f3` |
| Secondary text | `#5c626c` | `#bfc5ce` |
| Divider | `#cdd1d6` | `#515862` |
| Stage | `#747b82` | `#1c2026` |
| Accent | `#1973c8` | `#76b8ff` |
| Selected row | `#dcecff` | `#244967` |

Text contrast, calculated as WCAG 2.x relative luminance ratio: light primary on panel 15.0:1; light secondary on panel 5.8:1; dark primary on panel 9.9:1; dark secondary on panel 6.5:1. Selected-row and disabled text need a separate check in the implemented app. System colors should replace the fixed values where native materials or increased contrast settings require them.
