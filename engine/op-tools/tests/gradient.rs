use op_core::doc::{DocState, ToolCompositeMode};
use op_core::tiles::{TileCoord, TileStore};
use op_tools::gradient::{GradientMode, GradientOptions, GradientSession, GradientType, Handle};
use std::sync::Arc;

fn doc() -> DocState {
    let d = DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 20).unwrap()),
        5,
        5,
        96.0,
    )
    .unwrap();
    let mut tile = vec![0; 256 * 256 * 4];
    for p in tile.chunks_exact_mut(4) {
        p.copy_from_slice(&[40, 80, 120, 200]);
    }
    d.write_tile(
        0,
        TileCoord { x: 0, y: 0 },
        op_core::tiles::TileData::try_from(tile).unwrap(),
    )
    .unwrap()
}
fn options(shape: GradientType, mode: GradientMode) -> GradientOptions {
    GradientOptions {
        shape,
        mode,
        primary: [200, 20, 10, 128],
        secondary: [10, 220, 250, 64],
        antialias: false,
        blend: ToolCompositeMode::Overwrite,
    }
}
fn pixel(d: &DocState, x: usize, y: usize) -> [u8; 4] {
    let t = d.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    t[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4]
        .try_into()
        .unwrap()
}
#[test]
fn all_shapes_and_modes_are_finite_and_change_pixels() {
    for shape in [
        GradientType::Linear,
        GradientType::LinearReflected,
        GradientType::Diamond,
        GradientType::Radial,
        GradientType::Conical,
    ] {
        for mode in [GradientMode::Color, GradientMode::Transparency] {
            let s =
                GradientSession::new(doc(), options(shape, mode), (1.0, 1.0), (3.0, 1.0)).unwrap();
            let p = s.preview().unwrap();
            let (x, y) = if shape == GradientType::Conical {
                (1, 3)
            } else {
                (3, 1)
            };
            assert_ne!(pixel(&p, 1, 1), pixel(&p, x, y), "{shape:?} {mode:?}");
            if mode == GradientMode::Transparency {
                assert_eq!(&pixel(&p, 2, 1)[..3], &[40, 80, 120]);
            }
            let zero = GradientSession::new(doc(), options(shape, mode), (2.0, 2.0), (2.0, 2.0))
                .unwrap()
                .preview()
                .unwrap();
            // Coincident handles give the end value: secondary alpha in Color mode,
            // 255 - secondary alpha in Transparency mode (OpenPDN alpha-only rule).
            let end_alpha = if mode == GradientMode::Transparency {
                255 - 64
            } else {
                64
            };
            assert_eq!(pixel(&zero, 2, 2)[3], end_alpha);
            let reversed =
                GradientSession::new(doc(), options(shape, mode), (3.0, 1.0), (1.0, 1.0))
                    .unwrap()
                    .preview()
                    .unwrap();
            assert_eq!(
                pixel(&reversed, 3, 1)[3],
                if shape == GradientType::Conical {
                    end_alpha
                } else {
                    128
                }
            );
            let mut mask = vec![0; 25];
            mask[8] = 128;
            let selected = doc().set_selection(op_core::doc::SelectionState::Active(
                op_core::doc::SelectionSource::Coverage(Arc::new(
                    op_core::doc::MaskGrid::new(5, 5, mask).unwrap(),
                )),
            ));
            let clipped =
                GradientSession::new(selected, options(shape, mode), (1.0, 1.0), (3.0, 1.0))
                    .unwrap()
                    .preview()
                    .unwrap();
            assert_eq!(pixel(&clipped, 1, 1), [40, 80, 120, 200]);
            assert_ne!(pixel(&clipped, 3, 1), [40, 80, 120, 200]);
        }
    }
}
#[test]
fn linear_ported_arithmetic_and_reversed_handles() {
    let a = GradientSession::new(
        doc(),
        options(GradientType::Linear, GradientMode::Color),
        (1.0, 1.0),
        (3.0, 1.0),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(pixel(&a, 1, 1), [200, 20, 10, 128]);
    assert_eq!(pixel(&a, 3, 1), [10, 220, 250, 64]);
    let mid = pixel(&a, 2, 1);
    for (got, want) in mid.into_iter().zip([136, 86, 90, 96]) {
        assert!(got.abs_diff(want) <= 1);
    }
    let b = GradientSession::new(
        doc(),
        options(GradientType::Linear, GradientMode::Color),
        (3.0, 1.0),
        (1.0, 1.0),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(pixel(&b, 1, 1), [10, 220, 250, 64]);
}
#[test]
fn handle_updates_preview_without_changing_base() {
    let d = doc();
    let mut s = GradientSession::new(
        d.clone(),
        options(GradientType::Linear, GradientMode::Color),
        (1.0, 1.0),
        (3.0, 1.0),
    )
    .unwrap();
    s.update(Handle::End, (4.0, 1.0)).unwrap();
    s.update(Handle::Start, (0.0, 1.0)).unwrap();
    let preview = s.preview().unwrap();
    assert_eq!(pixel(&preview, 2, 1), pixel(&s.finish().unwrap(), 2, 1));
    assert_eq!(pixel(&d, 2, 1), [40, 80, 120, 200]);
}

#[test]
fn selection_geometry_and_soft_mask_clip_once() {
    use op_core::doc::{MaskGrid, PolygonSet, SelectionSource, SelectionState};
    let base = doc();
    let geometry = base
        .clone()
        .set_selection(SelectionState::Active(SelectionSource::Geometry(Arc::new(
            PolygonSet::rectangle(1.0, 1.0, 2.0, 2.0),
        ))));
    let output = GradientSession::new(
        geometry,
        options(GradientType::Linear, GradientMode::Color),
        (0.0, 0.0),
        (4.0, 0.0),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(pixel(&output, 0, 0), [40, 80, 120, 200]);
    assert_ne!(pixel(&output, 1, 1), [40, 80, 120, 200]);
    let mut mask = vec![0; 25];
    mask[7] = 128;
    mask[8] = 255;
    let selected = base.set_selection(SelectionState::Active(SelectionSource::Coverage(Arc::new(
        MaskGrid::new(5, 5, mask).unwrap(),
    ))));
    let output = GradientSession::new(
        selected,
        options(GradientType::Linear, GradientMode::Color),
        (1.0, 1.0),
        (3.0, 1.0),
    )
    .unwrap()
    .preview()
    .unwrap();
    let full = GradientSession::new(
        doc(),
        options(GradientType::Linear, GradientMode::Color),
        (1.0, 1.0),
        (3.0, 1.0),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(pixel(&output, 1, 1), [40, 80, 120, 200]);
    assert_eq!(pixel(&output, 3, 1), pixel(&full, 3, 1));
    let half = pixel(&output, 2, 1);
    let target = pixel(&full, 2, 1);
    for c in 0..4 {
        assert!(
            half[c].abs_diff(
                ((u16::from([40u8, 80, 120, 200][c]) * 127 + u16::from(target[c]) * 128 + 127)
                    / 255) as u8
            ) <= 1
        );
    }
}

#[test]
fn golden_every_shape_and_mode() {
    for shape in [
        GradientType::Linear,
        GradientType::LinearReflected,
        GradientType::Diamond,
        GradientType::Radial,
        GradientType::Conical,
    ] {
        for mode in [GradientMode::Color, GradientMode::Transparency] {
            let mut o = options(shape, mode);
            o.antialias = true;
            let d = GradientSession::new(doc(), o, (1.0, 1.0), (3.0, 2.0))
                .unwrap()
                .preview()
                .unwrap();
            let mut raw = vec![0; 5 * 5 * 4];
            for y in 0..5 {
                for x in 0..5 {
                    raw[(y * 5 + x) * 4..(y * 5 + x) * 4 + 4].copy_from_slice(&pixel(&d, x, y));
                }
            }
            let mut bytes = Vec::new();
            {
                let mut e = png::Encoder::new(&mut bytes, 5, 5);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().unwrap().write_image_data(&raw).unwrap();
            }
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/goldens/gradient")
                .join(format!("{shape:?}_{mode:?}.png"));
            if std::env::var_os("OP_BLESS").is_some() {
                std::fs::write(path, bytes).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn blend_modes_and_antialias_change_output() {
    for mode in [GradientMode::Color, GradientMode::Transparency] {
        let overwrite = options(GradientType::Linear, mode);
        let mut normal = overwrite;
        normal.blend = ToolCompositeMode::Normal;
        let a = GradientSession::new(doc(), overwrite, (1.0, 1.0), (3.0, 1.0))
            .unwrap()
            .preview()
            .unwrap();
        let b = GradientSession::new(doc(), normal, (1.0, 1.0), (3.0, 1.0))
            .unwrap()
            .preview()
            .unwrap();
        assert_ne!(pixel(&a, 1, 1), pixel(&b, 1, 1));
        if mode == GradientMode::Transparency {
            assert_eq!(pixel(&b, 1, 1), [40, 80, 120, 100]);
        }
    }
    let no_aa = options(GradientType::Radial, GradientMode::Color);
    let mut aa = no_aa;
    aa.antialias = true;
    let a = GradientSession::new(doc(), no_aa, (1.0, 1.0), (3.0, 2.0))
        .unwrap()
        .preview()
        .unwrap();
    let b = GradientSession::new(doc(), aa, (1.0, 1.0), (3.0, 2.0))
        .unwrap()
        .preview()
        .unwrap();
    assert_ne!(
        a.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap(),
        b.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap()
    );
}

#[test]
fn transparency_mode_ends_at_inverse_secondary_alpha() {
    // OpenPDN GradientRenderer.ComputeAlphaOnlyValuesFromColors:
    // start alpha = primary.A, end alpha = 255 - secondary.A.
    let p = GradientSession::new(
        doc(),
        options(GradientType::Linear, GradientMode::Transparency),
        (1.0, 1.0),
        (3.0, 1.0),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(pixel(&p, 1, 1), [40, 80, 120, 128]);
    assert_eq!(pixel(&p, 3, 1), [40, 80, 120, 255 - 64]);
}
