//! sims/population.rs against upstream's own snapshots: every snapshot's
//! sims were laid out by upstream's rebuildRuntimeSims at the last
//! placement and only changed in place since, so rebuilding them from the
//! snapshot's placed objects (with the snapshot's sims as the "previous"
//! ones) must give back exactly the same list, in the same order.  That
//! checks the allocation order (floor sort, first-fit, the ten-entry
//! placement queue and the medical blocks) on real towers.
//!
//! The dumps are the ones snapshot_roundtrip.rs reads (TOWER_SNAPSHOTS);
//! without them the test says so and passes.

use std::fs;
use std::path::PathBuf;

use tower_sim::sims::population::{find_object_for_sim, rebuild_runtime_sims};
use tower_sim::snapshot::SimSnapshot;

#[test]
fn rebuild_reproduces_upstream_sim_tables() {
    let dir = PathBuf::from(
        std::env::var("TOWER_SNAPSHOTS").unwrap_or_else(|_| "/Volumes/xc/BOX/caches/tower/snapshots".into()),
    );
    let Ok(entries) = fs::read_dir(&dir) else {
        eprintln!("SKIPPED: no snapshot dumps in {}", dir.display());
        return;
    };
    let mut files: Vec<PathBuf> = Vec::new();
    for fixture in entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()) {
        for f in fs::read_dir(&fixture).unwrap().filter_map(Result::ok).map(|e| e.path()) {
            if f.extension().is_some_and(|x| x == "json") {
                files.push(f);
            }
        }
    }
    files.sort();
    let mut failures = Vec::new();
    let mut skipped = 0;
    for file in &files {
        let snapshot: SimSnapshot = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
        // After events.ts has burnt objects away (the fire fixture) the sims
        // of those objects stay in the table until the next rebuild, which
        // would drop them: such a snapshot is not a rebuild's output.
        let orphans = (0..snapshot.world.sims.len() as i64)
            .filter(|&i| find_object_for_sim(&snapshot.world, i).is_none())
            .count();
        if orphans > 0 {
            eprintln!("{}: {orphans} sims without a home object, not compared", file.display());
            skipped += 1;
            continue;
        }
        let mut world = snapshot.world.clone();
        rebuild_runtime_sims(&mut world);
        if world.sims != snapshot.world.sims {
            let first = world
                .sims
                .iter()
                .zip(&snapshot.world.sims)
                .position(|(a, b)| a != b)
                .unwrap_or(world.sims.len().min(snapshot.world.sims.len()));
            failures.push(format!(
                "{}: {} sims rebuilt, {} saved, first difference at {first}",
                file.display(),
                world.sims.len(),
                snapshot.world.sims.len()
            ));
        }
    }
    eprintln!("{} snapshots checked, {skipped} skipped", files.len() - skipped);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
