//! `doc.profile_icc` (M17): the document's ICC bytes, for Mac export through ImageIO.
use op_engine::Engine;
use serde_json::json;

#[test]
fn doc_profile_is_empty_for_untagged_and_the_icc_bytes_when_tagged() {
    let e = Engine::new(None).unwrap();
    let plain = e.import_pixels(1, 1, &[1, 2, 3, 255], None).unwrap();
    assert!(e.query(plain, "doc.profile_icc", json!({})).unwrap().is_empty());

    let p3 = op_color::Profile::builtin("display-p3").unwrap().bytes();
    let tagged = e.import_pixels(1, 1, &[1, 2, 3, 255], Some(&p3)).unwrap();
    assert_eq!(
        e.query(tagged, "doc.profile_icc", json!({})).unwrap(),
        p3.to_vec()
    );

    // The query follows Image > Assign Profile.
    e.dispatch(
        plain,
        "image.assign_profile",
        json!({"profile":"display-p3"}),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while e.query(plain, "doc.profile_icc", json!({})).unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "assign_profile never published"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        e.query(plain, "doc.profile_icc", json!({})).unwrap(),
        p3.to_vec()
    );
}
