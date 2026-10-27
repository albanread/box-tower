//! The data-model check: every snapshot upstream's own simulation wrote
//! (harness/dump-snapshots.mjs) deserialises into `SimSnapshot` and
//! serialises back to the same JSON value, with the maps (`JsObject`s) in
//! the same key order.  That proves each field's name, type (integer or
//! not), optionality and enum tag.  It also checks that normalize_snapshot
//! leaves upstream's own current snapshots as they are.
//!
//! The dumps live outside the repository: TOWER_SNAPSHOTS, default
//! /Volumes/xc/BOX/caches/tower/snapshots.  Without them the test says so
//! and passes, unless TOWER_REQUIRE_SNAPSHOTS is set.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tower_sim::snapshot::{normalize_snapshot, SimSnapshot};

/// The world's fields that are `JsObject`s: their key order is upstream's
/// property order and must come back the same.
const ORDERED_MAPS: &[&str] = &[
    "$.world.cells",
    "$.world.cellToAnchor",
    "$.world.overlays",
    "$.world.overlayToAnchor",
    "$.world.placedObjects",
    "$.world.currentPopulationBuckets",
];

/// The first difference between two JSON values, as a path and a reason.
fn diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            if ORDERED_MAPS.contains(&path) {
                let kx: Vec<&String> = x.keys().collect();
                let ky: Vec<&String> = y.keys().collect();
                if kx != ky {
                    let i = kx.iter().zip(&ky).position(|(p, q)| p != q).unwrap_or(kx.len().min(ky.len()));
                    return Some(format!(
                        "{path}: key order differs at position {i}: {:?} vs {:?}",
                        kx.get(i),
                        ky.get(i)
                    ));
                }
            }
            for (k, v) in x {
                match y.get(k) {
                    None => return Some(format!("{path}.{k}: missing after the round trip")),
                    Some(w) => {
                        if let Some(d) = diff(v, w, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    }
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map(|k| format!("{path}.{k}: added by the round trip"))
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: length {} vs {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (v, w))| diff(v, w, &format!("{path}[{i}]")))
        }
        // Numbers compare by value and kind: an integer must come back an
        // integer (serde_json keeps 3 and 3.0 apart).
        _ if a == b => None,
        _ => Some(format!("{path}: {a} vs {b}")),
    }
}

#[test]
fn upstream_snapshots_round_trip() {
    let dir = PathBuf::from(
        std::env::var("TOWER_SNAPSHOTS").unwrap_or_else(|_| "/Volumes/xc/BOX/caches/tower/snapshots".into()),
    );
    let Ok(entries) = fs::read_dir(&dir) else {
        assert!(
            std::env::var_os("TOWER_REQUIRE_SNAPSHOTS").is_none(),
            "no snapshot dumps in {} (run harness/dump-snapshots.mjs)",
            dir.display()
        );
        eprintln!("SKIPPED: no snapshot dumps in {} (run harness/dump-snapshots.mjs)", dir.display());
        return;
    };
    let mut fixtures: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "no fixtures in {}", dir.display());

    let mut failures = Vec::new();
    let mut total = 0;
    for fixture in &fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let mut files: Vec<PathBuf> = fs::read_dir(fixture)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        let mut ok = 0;
        for file in &files {
            let label = format!("{name}/{}", file.file_name().unwrap().to_string_lossy());
            let text = fs::read_to_string(file).unwrap();
            let original: Value = serde_json::from_str(&text).unwrap();
            let snapshot: SimSnapshot = match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{label}: does not deserialise: {e}"));
                    continue;
                }
            };
            let back = serde_json::to_value(&snapshot).unwrap();
            if let Some(d) = diff(&original, &back, "$") {
                failures.push(format!("{label}: {d}"));
                continue;
            }
            // normalize_snapshot is the identity on upstream's own snapshots.
            let mut normalized = original.clone();
            if let Err(e) = normalize_snapshot(&mut normalized) {
                failures.push(format!("{label}: normalize_snapshot: {e}"));
                continue;
            }
            match serde_json::from_value::<SimSnapshot>(normalized) {
                Ok(n) if n == snapshot => {}
                Ok(n) => {
                    let d = diff(&back, &serde_json::to_value(&n).unwrap(), "$");
                    failures.push(format!("{label}: normalize_snapshot changed it: {d:?}"));
                    continue;
                }
                Err(e) => {
                    failures.push(format!("{label}: normalized snapshot does not deserialise: {e}"));
                    continue;
                }
            }
            ok += 1;
        }
        total += ok;
        eprintln!("{name}: {ok}/{} snapshots round-trip", files.len());
    }
    eprintln!("{total} snapshots from {} fixtures", fixtures.len());
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
