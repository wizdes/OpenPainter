use op_core::doc::{DocState, SelectionSource, SelectionState, ToolCompositeMode};
use op_core::selection::MaskGrid;
use op_core::tiles::{TileCoord, TileStore};
use op_tools::line::{Dash, Join, Point};
use op_tools::shape::{DrawMode, ShapeOptions, ShapeSession, catalog, geometry};
use std::sync::Arc;

fn base() -> DocState {
    DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 20).unwrap()),
        40,
        40,
        96.0,
    )
    .unwrap()
}
fn options(mode: DrawMode) -> ShapeOptions {
    ShapeOptions {
        mode,
        width: 2.0,
        antialias: true,
        dash: Dash::Solid,
        join: Join::Round,
        miter_limit: 4.0,
        primary: [240, 40, 20, 255],
        secondary: [20, 100, 240, 255],
        blend: ToolCompositeMode::Normal,
    }
}
fn bytes(s: &DocState) -> Vec<u8> {
    let tile = s.read_tile(0, TileCoord { x: 0, y: 0 }).unwrap();
    let mut out = Vec::new();
    for y in 0..40 {
        for x in 0..40 {
            out.extend_from_slice(&tile[(y * 256 + x) * 4..][..4]);
        }
    }
    out
}
fn golden(name: &str, rgba: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/goldens/shapes")
        .join(format!("{name}.png"));
    if std::env::var_os("OP_BLESS").is_some() {
        let mut data = Vec::new();
        let mut encoder = png::Encoder::new(&mut data, 40, 40);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgba)
            .unwrap();
        std::fs::write(&path, data).unwrap();
    }
    let mut decoder = png::Decoder::new(std::fs::File::open(path).unwrap())
        .read_info()
        .unwrap();
    let mut decoded = vec![0; decoder.output_buffer_size()];
    let frame = decoder.next_frame(&mut decoded).unwrap();
    assert_eq!(&decoded[..frame.buffer_size()], rgba);
}
#[test]
fn all_shapes_have_geometry_and_three_goldens() {
    assert_eq!(catalog().len(), 29);
    let expected_counts = [
        4, 36, 64, 4, 4, 4, 3, 3, 5, 6, 7, 8, 6, 8, 10, 12, 7, 8, 5, 6, 7, 11, 9, 33, 7, 6, 12, 32,
        64,
    ];
    for (entry, expected) in catalog().iter().zip(expected_counts) {
        let path = geometry(entry.id);
        assert_eq!(entry.vertex_count, expected, "{}", entry.id);
        assert!(path.len() >= 4, "{}", entry.id);
        assert_eq!(path.first(), path.last(), "{}", entry.id);
        assert!(
            path.iter()
                .all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)),
            "{}",
            entry.id
        );
        assert_eq!(path.len(), entry.vertex_count + 1, "{}", entry.id);
        for (name, mode) in [
            ("outline", DrawMode::Outline),
            ("fill", DrawMode::Fill),
            ("both", DrawMode::FillAndOutline),
        ] {
            assert_eq!(path.len(), expected + 1, "{} {name}", entry.id);
            assert_eq!(path.first(), path.last(), "{} {name}", entry.id);
            assert!(
                path.iter()
                    .all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)),
                "{} {name}",
                entry.id
            );
            if matches!(
                entry.id,
                "rectangle"
                    | "rounded_rectangle"
                    | "ellipse"
                    | "diamond"
                    | "trapezoid"
                    | "triangle"
                    | "pentagon"
                    | "hexagon"
                    | "heptagon"
                    | "octagon"
                    | "star_3"
                    | "star_4"
                    | "star_5"
                    | "star_6"
                    | "multiply"
                    | "gear"
                    | "heart"
            ) {
                for p in &path {
                    assert!(
                        path.iter()
                            .any(|q| (q.x - (1.0 - p.x)).abs() < 1e-9 && (q.y - p.y).abs() < 1e-9),
                        "{} {name} is not symmetric",
                        entry.id
                    );
                }
            }
            if matches!(
                entry.id,
                "rectangle"
                    | "rounded_rectangle"
                    | "ellipse"
                    | "diamond"
                    | "arrow"
                    | "notched_arrow"
                    | "pentagon_arrow"
                    | "chevron_arrow"
                    | "multiply"
                    | "gear"
            ) {
                for p in &path {
                    assert!(
                        path.iter()
                            .any(|q| (q.y - (1.0 - p.y)).abs() < 1e-9 && (q.x - p.x).abs() < 1e-9),
                        "{} {name} is not horizontally symmetric",
                        entry.id
                    );
                }
            }
            let session = ShapeSession::new(
                base(),
                entry.id,
                [Point::new(5.0, 5.0), Point::new(35.0, 35.0)],
                0.0,
                options(mode),
            )
            .unwrap();
            let image = bytes(&session.preview().unwrap());
            golden(&format!("{}_{}", entry.id, name), &image);
        }
    }
}
#[test]
fn zero_reverse_rotate_update_and_selection() {
    let mut s = ShapeSession::new(
        base(),
        "heart",
        [Point::new(30.0, 30.0), Point::new(8.0, 6.0)],
        0.0,
        options(DrawMode::FillAndOutline),
    )
    .unwrap();
    let first = bytes(&s.preview().unwrap());
    s.update(
        Some("rectangle"),
        Some([Point::new(8.0, 6.0), Point::new(30.0, 30.0)]),
        Some(0.5),
        None,
    )
    .unwrap();
    assert_ne!(first, bytes(&s.preview().unwrap()));
    assert_eq!(bytes(&s.preview().unwrap()), bytes(&s.finish().unwrap()));
    let forward = ShapeSession::new(
        base(),
        "rectangle",
        [Point::new(8.0, 6.0), Point::new(30.0, 30.0)],
        0.0,
        options(DrawMode::Fill),
    )
    .unwrap();
    let reverse = ShapeSession::new(
        base(),
        "rectangle",
        [Point::new(30.0, 30.0), Point::new(8.0, 6.0)],
        0.0,
        options(DrawMode::Fill),
    )
    .unwrap();
    assert_eq!(
        bytes(&forward.preview().unwrap()),
        bytes(&reverse.preview().unwrap())
    );
    let rotated = ShapeSession::new(
        base(),
        "rectangle",
        [Point::new(8.0, 6.0), Point::new(30.0, 30.0)],
        0.5,
        options(DrawMode::Fill),
    )
    .unwrap();
    assert_ne!(
        bytes(&forward.preview().unwrap()),
        bytes(&rotated.preview().unwrap())
    );
    let zero = ShapeSession::new(
        base(),
        "rectangle",
        [Point::new(10.0, 10.0); 2],
        0.0,
        options(DrawMode::Fill),
    )
    .unwrap();
    assert!(bytes(&zero.preview().unwrap()).iter().all(|b| *b == 0));
    let mut coverage = vec![0; 40 * 40];
    coverage[20 * 40 + 20] = 128;
    let selected = base().set_selection(SelectionState::Active(SelectionSource::Coverage(
        Arc::new(MaskGrid::new(40, 40, coverage).unwrap()),
    )));
    let s = ShapeSession::new(
        selected,
        "rectangle",
        [Point::new(5.0, 5.0), Point::new(35.0, 35.0)],
        0.0,
        options(DrawMode::Fill),
    )
    .unwrap();
    let pixels = bytes(&s.preview().unwrap());
    assert_eq!(pixels.chunks_exact(4).filter(|p| p[3] > 0).count(), 1);
    assert!(pixels[(20 * 40 + 20) * 4 + 3] <= 128);
    let fill = ShapeSession::new(
        base(),
        "rectangle",
        [Point::new(5.0, 5.0), Point::new(35.0, 35.0)],
        0.0,
        options(DrawMode::Fill),
    )
    .unwrap();
    let p = bytes(&fill.preview().unwrap());
    assert_eq!(&p[(20 * 40 + 20) * 4..][..4], &[240, 40, 20, 255]);
}
#[test]
fn closed_outline_joins_first_vertex() {
    let mut style = options(DrawMode::Outline);
    style.width = 8.0;
    style.join = Join::Miter;
    let shape = ShapeSession::new(
        base(),
        "diamond",
        [Point::new(10.0, 10.0), Point::new(30.0, 30.0)],
        0.0,
        style,
    )
    .unwrap();
    let pixels = bytes(&shape.preview().unwrap());
    assert!(pixels[(5 * 40 + 20) * 4 + 3] > 0);
}
#[test]
fn cloud_callout_tail_stays_on_nearby_boundary() {
    let path = geometry("cloud_callout");
    for pair in path.windows(2) {
        let length = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
        assert!(
            length < 0.6,
            "cloud callout edge crosses the body: {length}"
        );
    }
}
