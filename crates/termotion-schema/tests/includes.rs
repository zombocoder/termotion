use std::fs;
use std::path::Path;

use termotion_schema::diag::codes;
use termotion_schema::include::load_merged;

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("termotion-include-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn including_file_overrides_its_includes() {
    let dir = tempdir("override");
    write(&dir, "base.yaml", "canvas:\n  width: 1280\n  height: 720\n");
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./base.yaml\ncanvas:\n  width: 1920\n",
    );

    let merged = load_merged(&dir.join("scene.yaml")).unwrap();
    assert_eq!(merged["canvas"]["width"].as_u64(), Some(1920));
    assert_eq!(merged["canvas"]["height"].as_u64(), Some(720));
}

#[test]
fn later_includes_win_over_earlier_ones() {
    let dir = tempdir("order");
    write(&dir, "a.yaml", "canvas:\n  fps: 30\n");
    write(&dir, "b.yaml", "canvas:\n  fps: 60\n");
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./a.yaml\n  - ./b.yaml\n",
    );

    let merged = load_merged(&dir.join("scene.yaml")).unwrap();
    assert_eq!(merged["canvas"]["fps"].as_u64(), Some(60));
}

#[test]
fn timelines_concatenate_with_includes_first() {
    let dir = tempdir("timeline");
    write(
        &dir,
        "header.yaml",
        "timeline:\n  - type: write\n    text: header\n",
    );
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./header.yaml\ntimeline:\n  - type: write\n    text: body\n",
    );

    let merged = load_merged(&dir.join("scene.yaml")).unwrap();
    let timeline = merged["timeline"].as_sequence().unwrap();
    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0]["text"].as_str(), Some("header"));
    assert_eq!(timeline[1]["text"].as_str(), Some("body"));
}

#[test]
fn includes_resolve_relative_to_the_including_file() {
    let dir = tempdir("relative");
    fs::create_dir_all(dir.join("common")).unwrap();
    write(
        &dir.join("common"),
        "prompt.yaml",
        "prompt:\n  user: zombocoder\n",
    );
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./common/prompt.yaml\n",
    );

    let merged = load_merged(&dir.join("scene.yaml")).unwrap();
    assert_eq!(merged["prompt"]["user"].as_str(), Some("zombocoder"));
}

#[test]
fn include_cycles_are_detected() {
    let dir = tempdir("cycle");
    write(&dir, "a.yaml", "includes:\n  - ./b.yaml\n");
    write(&dir, "b.yaml", "includes:\n  - ./a.yaml\n");
    write(&dir, "scene.yaml", "version: 1\nincludes:\n  - ./a.yaml\n");

    let err = load_merged(&dir.join("scene.yaml")).unwrap_err();
    assert_eq!(err.code, codes::INCLUDE_CYCLE);
}

#[test]
fn missing_includes_report_the_path() {
    let dir = tempdir("missing");
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./nope.yaml\n",
    );

    let err = load_merged(&dir.join("scene.yaml")).unwrap_err();
    assert_eq!(err.code, codes::INCLUDE_NOT_FOUND);
    assert!(err.message.contains("nope.yaml"));
}

#[test]
fn the_includes_key_is_removed_after_merging() {
    let dir = tempdir("stripped");
    write(&dir, "base.yaml", "canvas:\n  fps: 30\n");
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./base.yaml\n",
    );

    let merged = load_merged(&dir.join("scene.yaml")).unwrap();
    assert!(merged.get("includes").is_none());
}

#[test]
fn include_chains_deeper_than_the_cap_are_rejected() {
    let dir = tempdir("deep");

    // scene.yaml (depth 0) -> level1 (depth 1) -> ... -> level8 (depth 8,
    // which meets MAX_INCLUDE_DEPTH == 8 and is rejected: the cap counts the
    // root scenario itself, so 8 total files -- root plus 7 includes -- may
    // load, and an 8th include may not).
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./level1.yaml\n",
    );
    for level in 1..=7 {
        let next = level + 1;
        write(
            &dir,
            &format!("level{level}.yaml"),
            &format!("includes:\n  - ./level{next}.yaml\n"),
        );
    }
    // level8.yaml exists (so the existence check on the way in doesn't
    // pre-empt things) but is never read: the depth check must fire first,
    // at depth 8, before the file's contents are parsed or recursed into.
    write(&dir, "level8.yaml", "version: 1\n");

    let err = load_merged(&dir.join("scene.yaml")).unwrap_err();
    assert_eq!(err.code, codes::INCLUDE_TOO_DEEP);
}

#[test]
fn an_include_chain_exactly_at_the_cap_succeeds() {
    let dir = tempdir("at-cap");

    // scene.yaml (depth 0) -> level1 (depth 1) -> ... -> level6 (depth 6) ->
    // level7 (depth 7): 8 total files, the deepest chain MAX_INCLUDE_DEPTH
    // permits.
    write(
        &dir,
        "scene.yaml",
        "version: 1\nincludes:\n  - ./level1.yaml\n",
    );
    for level in 1..=6 {
        let next = level + 1;
        write(
            &dir,
            &format!("level{level}.yaml"),
            &format!("includes:\n  - ./level{next}.yaml\n"),
        );
    }
    write(&dir, "level7.yaml", "canvas:\n  fps: 24\n");

    let merged = load_merged(&dir.join("scene.yaml")).unwrap();
    assert_eq!(merged["canvas"]["fps"].as_u64(), Some(24));
}
