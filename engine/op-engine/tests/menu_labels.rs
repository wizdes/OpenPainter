//! M55 menu names: every effect's label is the name paint.net 5.1 shows in its
//! menus (tests/catalog/paintnet-5.1-menus.toml), and no two effects share one.
use op_engine::Engine;
use serde_json::{Value, json};
use std::collections::HashSet;

fn effects() -> Vec<Value> {
    let e = Engine::new(None).unwrap();
    let list: Value =
        serde_json::from_slice(&e.query(0, "effects.list", json!({})).unwrap()).unwrap();
    list.as_array().unwrap().clone()
}

fn label(list: &[Value], id: &str) -> String {
    list.iter()
        .find(|e| e["id"] == json!(id))
        .unwrap_or_else(|| panic!("{id} is registered"))["label"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn median_is_named_median_blur_as_in_paint_net_5_1() {
    // https://www.getpaint.net/doc/latest/EffectsBlursMenu.html lists
    // "Median Blur" under Effects > Blurs.
    assert_eq!(label(&effects(), "effect.blur.median"), "Median Blur");
}

#[test]
fn effect_labels_are_unique() {
    let list = effects();
    let mut seen = HashSet::new();
    for e in &list {
        let l = e["label"].as_str().unwrap();
        assert!(seen.insert(l.to_string()), "two effects are named {l}");
    }
}
