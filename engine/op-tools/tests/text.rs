// Text session compositing. The runs here are fixed bytes, not fonts, so these
// goldens do not depend on the operating system.
use op_core::doc::{DocState, MaskGrid, SelectionSource, SelectionState, ToolCompositeMode};
use op_core::tiles::{TileCoord, TileData, TileStore};
use op_tools::text::{RunKind, TextModel, TextRun, TextSession, TextStyle, TextUpdate};
use std::sync::Arc;

fn doc(w: u32, h: u32, fill: [u8; 4]) -> DocState {
    let d = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 22).unwrap()),
        w,
        h,
        96.0,
    )
    .unwrap();
    let mut tile = vec![0; 256 * 256 * 4];
    for p in tile.chunks_exact_mut(4) {
        p.copy_from_slice(&fill);
    }
    let mut d = d;
    for ty in 0..h.div_ceil(256) {
        for tx in 0..w.div_ceil(256) {
            d = d
                .write_tile(
                    0,
                    TileCoord {
                        x: tx as i32,
                        y: ty as i32,
                    },
                    TileData::try_from(tile.clone()).unwrap(),
                )
                .unwrap();
        }
    }
    d
}
fn pixel(d: &DocState, x: usize, y: usize) -> [u8; 4] {
    let t = d
        .read_tile(
            d.active_layer,
            TileCoord {
                x: (x / 256) as i32,
                y: (y / 256) as i32,
            },
        )
        .unwrap();
    let k = ((y % 256) * 256 + x % 256) * 4;
    t[k..k + 4].try_into().unwrap()
}
fn all(d: &DocState) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..d.height as usize {
        for x in 0..d.width as usize {
            out.extend_from_slice(&pixel(d, x, y));
        }
    }
    out
}
fn coverage(x: i32, y: i32, w: u32, h: u32, data: Vec<u8>) -> TextRun {
    TextRun {
        kind: RunKind::Coverage,
        x,
        y,
        width: w,
        height: h,
        pixels: data.into(),
    }
}
fn color(x: i32, y: i32, w: u32, h: u32, data: Vec<u8>) -> TextRun {
    TextRun {
        kind: RunKind::Color,
        x,
        y,
        width: w,
        height: h,
        pixels: data.into(),
    }
}
fn style(primary: [u8; 4], blend: ToolCompositeMode) -> TextStyle {
    TextStyle { primary, blend }
}
/// FNV-1a, 64 bit: a stable digest for the golden.
fn hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[test]
fn coverage_run_paints_the_primary_color_at_the_origin() {
    let base = doc(4, 3, [0, 0, 255, 255]);
    let run = coverage(1, 0, 2, 1, vec![255, 128]);
    let s = TextSession::new(
        base,
        (0, 1),
        style([255, 0, 0, 255], ToolCompositeMode::Normal),
        vec![run],
        TextModel::default(),
    )
    .unwrap();
    let out = s.preview().unwrap();
    assert_eq!(pixel(&out, 1, 1), [255, 0, 0, 255]);
    assert_eq!(pixel(&out, 2, 1), [128, 0, 127, 255]);
    assert_eq!(pixel(&out, 0, 1), [0, 0, 255, 255]);
    assert_eq!(pixel(&out, 1, 0), [0, 0, 255, 255]);
    assert_eq!(s.bounds(), Some((1, 1, 2, 1)));
}

#[test]
fn runs_outside_the_canvas_are_clipped() {
    let s = TextSession::new(
        doc(2, 2, [0; 4]),
        (0, 0),
        style([255, 0, 0, 255], ToolCompositeMode::Normal),
        vec![coverage(-1, -1, 4, 4, vec![255; 16])],
        TextModel::default(),
    )
    .unwrap();
    assert_eq!(all(&s.preview().unwrap()), [255, 0, 0, 255].repeat(4));
    assert_eq!(s.bounds(), Some((0, 0, 2, 2)));
    let far = TextSession::new(
        doc(2, 2, [0; 4]),
        (0, 0),
        style([255, 0, 0, 255], ToolCompositeMode::Normal),
        vec![coverage(100, 100, 2, 2, vec![255; 4])],
        TextModel::default(),
    )
    .unwrap();
    assert_eq!(far.bounds(), None);
    assert_eq!(all(&far.preview().unwrap()), vec![0; 16]);
}

#[test]
fn bad_runs_and_carets_are_rejected() {
    let st = style([0, 0, 0, 255], ToolCompositeMode::Normal);
    let short = coverage(0, 0, 2, 2, vec![1, 2, 3]);
    assert!(
        TextSession::new(
            doc(2, 2, [0; 4]),
            (0, 0),
            st,
            vec![short],
            TextModel::default()
        )
        .is_err()
    );
    let not_premultiplied = color(0, 0, 1, 1, vec![200, 0, 0, 100]);
    assert!(
        TextSession::new(
            doc(2, 2, [0; 4]),
            (0, 0),
            st,
            vec![not_premultiplied],
            TextModel::default()
        )
        .is_err()
    );
    // "é" is two bytes in UTF-8, so byte offset 1 is inside it.
    assert!(TextModel::new("é".into(), 1).is_err());
    assert!(TextModel::new("é".into(), 2).is_ok());
    assert!(TextModel::new("a".into(), 2).is_err());
}

#[test]
fn color_run_keeps_its_own_colors_and_primary_alpha_scales_it() {
    // One opaque red pixel and one half-covered (premultiplied) green pixel.
    let run = color(0, 0, 2, 1, vec![255, 0, 0, 255, 0, 64, 0, 128]);
    let s = TextSession::new(
        doc(2, 1, [0, 0, 0, 0]),
        (0, 0),
        style([10, 20, 30, 255], ToolCompositeMode::Normal),
        vec![run.clone()],
        TextModel::default(),
    )
    .unwrap();
    let out = s.preview().unwrap();
    assert_eq!(pixel(&out, 0, 0), [255, 0, 0, 255], "not the primary color");
    assert_eq!(pixel(&out, 1, 0), [0, 128, 0, 128]);
    let half = TextSession::new(
        doc(2, 1, [0, 0, 0, 0]),
        (0, 0),
        style([10, 20, 30, 128], ToolCompositeMode::Normal),
        vec![run],
        TextModel::default(),
    )
    .unwrap();
    assert_eq!(pixel(&half.preview().unwrap(), 0, 0), [255, 0, 0, 128]);
}

#[test]
fn overwrite_replaces_inside_coverage_and_normal_blends() {
    let base = doc(2, 1, [0, 0, 255, 255]);
    let run = coverage(0, 0, 2, 1, vec![255, 0]);
    let translucent = [255, 0, 0, 64];
    let normal = TextSession::new(
        base.clone(),
        (0, 0),
        style(translucent, ToolCompositeMode::Normal),
        vec![run.clone()],
        TextModel::default(),
    )
    .unwrap()
    .preview()
    .unwrap();
    let over = TextSession::new(
        base,
        (0, 0),
        style(translucent, ToolCompositeMode::Overwrite),
        vec![run],
        TextModel::default(),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(
        pixel(&over, 0, 0),
        translucent,
        "Overwrite replaces alpha too"
    );
    assert_eq!(pixel(&normal, 0, 0)[3], 255, "Normal keeps the opaque base");
    assert_ne!(pixel(&normal, 0, 0), [0, 0, 255, 255]);
    assert_eq!(
        pixel(&over, 1, 0),
        [0, 0, 255, 255],
        "zero coverage leaves the pixel"
    );
}

#[test]
fn selection_coverage_clips_the_text() {
    let mut mask = vec![0; 4];
    mask[1] = 255;
    mask[2] = 128;
    let base = doc(2, 2, [0, 0, 0, 0]).set_selection(SelectionState::Active(
        SelectionSource::Coverage(Arc::new(MaskGrid::new(2, 2, mask).unwrap())),
    ));
    let s = TextSession::new(
        base,
        (0, 0),
        style([255, 255, 255, 255], ToolCompositeMode::Overwrite),
        vec![coverage(0, 0, 2, 2, vec![255; 4])],
        TextModel::default(),
    )
    .unwrap();
    let out = s.preview().unwrap();
    assert_eq!(pixel(&out, 0, 0), [0, 0, 0, 0], "outside the selection");
    assert_eq!(pixel(&out, 1, 0), [255, 255, 255, 255]);
    assert_eq!(
        pixel(&out, 0, 1),
        [128, 128, 128, 128],
        "half-selected pixel"
    );
    assert_eq!(pixel(&out, 1, 1), [0, 0, 0, 0]);
}

#[test]
fn updates_move_restyle_and_replace_runs_without_touching_the_base() {
    let base = doc(8, 4, [0, 0, 0, 255]);
    let before = all(&base);
    let mut s = TextSession::new(
        base.clone(),
        (0, 0),
        style([255, 0, 0, 255], ToolCompositeMode::Normal),
        vec![coverage(0, 0, 1, 1, vec![255])],
        TextModel::new("a".into(), 1).unwrap(),
    )
    .unwrap();
    s.update(TextUpdate {
        origin: Some((5, 2)),
        style: Some(style([0, 255, 0, 255], ToolCompositeMode::Normal)),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(pixel(&s.preview().unwrap(), 5, 2), [0, 255, 0, 255]);
    assert_eq!(pixel(&s.preview().unwrap(), 0, 0), [0, 0, 0, 255]);
    s.update(TextUpdate {
        runs: Some(vec![coverage(0, 0, 2, 1, vec![255, 255])]),
        model: Some(TextModel::new("ab".into(), 2).unwrap()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(pixel(&s.preview().unwrap(), 6, 2), [0, 255, 0, 255]);
    assert_eq!(s.model().text, "ab");
    assert_eq!(s.generation(), 2);
    // A bad update changes nothing.
    assert!(
        s.update(TextUpdate {
            runs: Some(vec![coverage(0, 0, 3, 1, vec![1])]),
            ..Default::default()
        })
        .is_err()
    );
    assert_eq!(s.runs().len(), 1);
    assert_eq!(all(&base), before);
}

#[test]
fn preview_equals_finish() {
    let s = TextSession::new(
        doc(300, 20, [9, 9, 9, 255]),
        (250, 3),
        style([200, 100, 50, 255], ToolCompositeMode::Normal),
        vec![
            coverage(0, 0, 10, 10, (0..100).map(|i| (i * 2) as u8).collect()),
            color(
                12,
                0,
                2,
                2,
                vec![255, 255, 255, 255, 0, 0, 0, 0, 10, 20, 30, 40, 0, 0, 0, 255],
            ),
        ],
        TextModel::default(),
    )
    .unwrap();
    let preview = all(&s.preview().unwrap());
    let done = all(&s.finish().unwrap());
    assert_eq!(preview, done);
}

/// Golden: fixed coverage and color runs over a gradient base, crossing a tile edge.
#[test]
fn golden_fixed_runs_over_a_tile_edge() {
    let mut base = doc(260, 6, [0; 4]);
    let mut tile0 = vec![0u8; 256 * 256 * 4];
    for y in 0..6 {
        for x in 0..256 {
            let k = (y * 256 + x) * 4;
            tile0[k..k + 4].copy_from_slice(&[x as u8, (y * 40) as u8, 128, 255]);
        }
    }
    base = base
        .write_tile(
            0,
            TileCoord { x: 0, y: 0 },
            TileData::try_from(tile0).unwrap(),
        )
        .unwrap();
    // A 6 x 4 "glyph": a ramp of coverage values.
    let cov: Vec<u8> = (0..24).map(|i| (i * 11) as u8).collect();
    // A 2 x 2 color run: opaque yellow, half cyan, clear, quarter magenta (premultiplied).
    let rgba = vec![255, 255, 0, 255, 0, 64, 64, 64, 0, 0, 0, 0, 64, 0, 64, 64];
    let s = TextSession::new(
        base,
        (252, 1),
        style([20, 40, 220, 200], ToolCompositeMode::Normal),
        vec![coverage(0, 0, 6, 4, cov), color(6, 1, 2, 2, rgba)],
        TextModel::new("Ab😀".into(), 2).unwrap(),
    )
    .unwrap();
    let out = all(&s.preview().unwrap());
    assert_eq!(
        pixel(&s.preview().unwrap(), 252, 1),
        [252, 40, 128, 255],
        "zero coverage"
    );
    assert_eq!(
        pixel(&s.preview().unwrap(), 258, 2),
        [255, 255, 0, 200],
        "opaque color pixel, scaled by the primary alpha, over a clear base"
    );
    assert_eq!(
        hex(&out),
        "6e6e13dca5809db0",
        "golden for fixed runs (update only with a reviewed reason)"
    );
}
