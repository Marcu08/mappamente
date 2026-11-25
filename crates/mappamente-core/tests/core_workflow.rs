//! End-to-end tests through the public crate API.

use std::time::{Duration, Instant};

use mappamente_core::io::json;
use mappamente_core::{Editor, Layout, LayoutKind, LayoutOptions, Metrics, MindMap, Result};

fn add_wide_tree(map: &mut MindMap, parent: &str, breadth: usize, depth: usize) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    for index in 0..breadth {
        let id = map.add_child(parent, format!("Branch {depth}-{index}"))?;
        add_wide_tree(map, &id, breadth, depth - 1)?;
    }
    Ok(())
}

#[test]
fn editor_layout_and_persistence_workflow() {
    let map = MindMap::new("Workflow", "Central idea");
    let mut editor = Editor::new(map);
    let root = editor.map().root.id.clone();

    editor
        .apply("generate branches", |map| {
            for branch in 0..5 {
                let branch_id = map.add_child(&root, format!("Branch {branch}"))?;
                for leaf in 0..3 {
                    map.add_child(&branch_id, format!("Leaf {branch}.{leaf}"))?;
                }
            }
            Ok(())
        })
        .unwrap();

    assert_eq!(editor.map().node_count(), 21);
    assert!(editor.is_dirty());

    let layout = Layout::compute(editor.map(), Metrics::default(), LayoutOptions::default());
    assert_eq!(layout.len(), 21);
    assert!(layout.bounds.w > 0.0);
    assert!(layout.bounds.h > 0.0);

    let directory = std::env::temp_dir().join(format!(
        "mappamente-workflow-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("workflow.mappa");
    json::save(&path, editor.map()).unwrap();

    let loaded = json::load(&path).unwrap();
    assert_eq!(&loaded, editor.map());
    assert!(loaded.outline().contains("- Branch 3\n    - Leaf 3.0"));
    assert!(loaded.outline().contains("    - Leaf 3.2"));

    let mut reopened = Editor::new(loaded);
    assert!(!reopened.is_dirty());
    assert!(
        reopened
            .apply("rename", |map| map.set_text(&root, "Renamed"))
            .is_ok()
    );
    assert!(reopened.is_dirty());
    assert!(reopened.undo());
    assert_eq!(reopened.map().root.text, "Central idea");

    editor.undo();
    assert_eq!(editor.map().node_count(), 1);
    assert!(editor.redo());

    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn thousand_node_radial_layout_completes_quickly_and_does_not_overlap() {
    let mut map = MindMap::new("Large", "Root");
    map.set_layout(LayoutKind::Radial);
    let root = map.root.id.clone();
    add_wide_tree(&mut map, &root, 10, 3).unwrap();
    assert_eq!(map.node_count(), 1111);

    let started = Instant::now();
    let layout = Layout::compute(&map, Metrics::default(), LayoutOptions::default());
    let elapsed = started.elapsed();
    println!("laid out {} nodes in {elapsed:?}", layout.len());

    assert_eq!(layout.len(), map.node_count());
    assert!(
        elapsed < Duration::from_secs(5),
        "layout took too long: {elapsed:?}"
    );

    for (index, first) in layout.nodes.iter().enumerate() {
        for second in &layout.nodes[index + 1..] {
            assert!(
                !first.rect.overlaps(second.rect, 0.01),
                "nodes {} and {} overlap",
                first.id,
                second.id
            );
        }
    }
}
