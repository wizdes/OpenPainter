use op_core::doc::{DocState, SelectionSource, SelectionState, ToolCompositeMode};
use op_core::selection::MaskGrid;
use op_core::tiles::TileStore;
use op_tools::line::{Cap, Dash, Join, LineKind, LineOptions, LineSession, Point, flatten};
use std::sync::Arc;

fn base() -> DocState {
    DocState::new(
        Arc::new(TileStore::new(std::env::temp_dir(), 1 << 20).unwrap()),
        32,
        32,
        96.0,
    )
    .unwrap()
}
fn options() -> LineOptions {
    LineOptions {
        kind: LineKind::Straight,
        width: 3.0,
        antialias: true,
        dash: Dash::Solid,
        start_cap: Cap::Flat,
        end_cap: Cap::Flat,
        join: Join::Miter,
        miter_limit: 4.0,
        primary: [200, 20, 10, 255],
        blend: ToolCompositeMode::Overwrite,
    }
}
fn bytes(s: &DocState) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..32 {
        for x in 0..32 {
            let tile = s
                .read_tile(0, op_core::tiles::TileCoord { x: 0, y: 0 })
                .unwrap();
            out.extend_from_slice(&tile[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4]);
        }
    }
    out
}
fn golden(name: &str, rgba: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/goldens/line")
        .join(format!("{name}.png"));
    if std::env::var_os("OP_BLESS").is_some() {
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, 32, 32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(rgba)
                .unwrap();
        }
        std::fs::write(&path, data).unwrap();
    }
    let file = std::fs::File::open(path).unwrap();
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut decoded).unwrap();
    assert_eq!(&decoded[..frame.buffer_size()], rgba);
}
#[test]
fn five_edits_regenerate_from_base() {
    let a = Point::new(4.0, 6.0);
    let b = Point::new(20.0, 20.0);
    let mut session = LineSession::new(base(), vec![a, b], options()).unwrap();
    for i in 0..5 {
        session
            .update_point(1, Point::new(21.0 + i as f64, 20.0))
            .unwrap();
        assert_eq!(session.generation(), i + 1);
    }
    let preview = session.preview().unwrap();
    let finished = session.finish().unwrap();
    let direct = LineSession::new(base(), vec![a, b], options())
        .unwrap()
        .preview()
        .unwrap();
    assert_ne!(bytes(&finished), bytes(&direct));
    let direct = LineSession::new(base(), vec![a, Point::new(25.0, 20.0)], options())
        .unwrap()
        .preview()
        .unwrap();
    assert_eq!(bytes(&finished), bytes(&preview));
    assert_eq!(bytes(&finished), bytes(&direct));
}
#[test]
fn flatten_is_in_image_pixels() {
    let pts = vec![
        Point::new(2.0, 10.0),
        Point::new(5.0, 29.0),
        Point::new(23.0, 1.0),
        Point::new(29.0, 20.0),
    ];
    let line = flatten(LineKind::Bezier, &pts).unwrap();
    for zoom in [0.25, 1.0, 4.0, 16.0] {
        let same = flatten(LineKind::Bezier, &pts).unwrap();
        assert_eq!(line, same, "zoom={zoom}");
    }
    for i in 0..=1000 {
        let t = i as f64 / 1000.0;
        let p = (0..4).fold(Point::new(0.0, 0.0), |acc, k| {
            let w = [1.0, 3.0, 3.0, 1.0][k] * (1.0 - t).powi((3 - k) as i32) * t.powi(k as i32);
            Point::new(acc.x + pts[k].x * w, acc.y + pts[k].y * w)
        });
        let distance = line
            .windows(2)
            .map(|s| {
                let v = Point::new(s[1].x - s[0].x, s[1].y - s[0].y);
                let u = Point::new(p.x - s[0].x, p.y - s[0].y);
                let t = ((u.x * v.x + u.y * v.y) / (v.x * v.x + v.y * v.y)).clamp(0.0, 1.0);
                (u.x - t * v.x).hypot(u.y - t * v.y)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(distance < 0.1, "distance={distance}");
    }
}
#[test]
fn reversed_and_zero_length_are_defined() {
    let p = Point::new(12.0, 12.0);
    let zero = LineSession::new(base(), vec![p, p], options())
        .unwrap()
        .preview()
        .unwrap();
    assert_eq!(bytes(&zero), bytes(&base()));
    let mut round = options();
    round.start_cap = Cap::Round;
    round.end_cap = Cap::Round;
    let dot = LineSession::new(base(), vec![p, p], round)
        .unwrap()
        .preview()
        .unwrap();
    assert_ne!(bytes(&dot), bytes(&base()));
    let forward = LineSession::new(
        base(),
        vec![Point::new(3.0, 7.0), Point::new(26.0, 7.0)],
        options(),
    )
    .unwrap()
    .preview()
    .unwrap();
    let backward = LineSession::new(
        base(),
        vec![Point::new(26.0, 7.0), Point::new(3.0, 7.0)],
        options(),
    )
    .unwrap()
    .preview()
    .unwrap();
    assert_eq!(bytes(&forward), bytes(&backward));
}
#[test]
fn sharp_join_and_short_dashes_have_goldens() {
    let mut miter = options();
    miter.kind = LineKind::Straight;
    miter.width = 5.0;
    miter.miter_limit = 1.0;
    let p = vec![
        Point::new(4.0, 25.0),
        Point::new(16.0, 4.0),
        Point::new(18.0, 26.0),
    ];
    let bevelled = LineSession::new(base(), p.clone(), miter)
        .unwrap()
        .preview()
        .unwrap();
    golden("sharp_miter_limit", &bytes(&bevelled));
    let mut bevel = miter;
    bevel.join = Join::Bevel;
    let bevel_pixels = LineSession::new(base(), p.clone(), bevel)
        .unwrap()
        .preview()
        .unwrap();
    assert_eq!(bytes(&bevelled), bytes(&bevel_pixels));
    let mut long_miter = miter;
    long_miter.miter_limit = 10.0;
    let long_pixels = LineSession::new(base(), p, long_miter)
        .unwrap()
        .preview()
        .unwrap();
    assert_ne!(bytes(&bevelled), bytes(&long_pixels));
    miter.dash = Dash::DashDotDot;
    miter.width = 5.0;
    miter.kind = LineKind::Straight;
    let dashed = LineSession::new(
        base(),
        vec![Point::new(2.0, 16.0), Point::new(30.0, 16.0)],
        miter,
    )
    .unwrap()
    .preview()
    .unwrap();
    golden("short_dashes", &bytes(&dashed));
    assert_ne!(bytes(&bevelled), bytes(&dashed));
}
#[test]
fn soft_selection_multiplies_line_coverage() {
    let mut coverage = vec![0; 32 * 32];
    coverage[16 * 32 + 16] = 128;
    let selected = base().set_selection(SelectionState::Active(SelectionSource::Coverage(
        Arc::new(MaskGrid::new(32, 32, coverage).unwrap()),
    )));
    let line = LineSession::new(
        selected,
        vec![Point::new(4.0, 16.5), Point::new(28.0, 16.5)],
        options(),
    )
    .unwrap()
    .preview()
    .unwrap();
    let pixels = bytes(&line);
    assert_eq!(pixels.chunks_exact(4).filter(|p| p[3] > 0).count(), 1);
    assert!(pixels[(16 * 32 + 16) * 4 + 3] > 0);
    assert!(pixels[(16 * 32 + 16) * 4 + 3] <= 128);
}
