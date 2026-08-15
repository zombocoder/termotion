use std::fs;
use std::sync::Mutex;

use termotion_schema::{load, resolve::Overrides};

/// Serializes tests that call `std::env::set_current_dir`. The process working
/// directory is global state; without this, cargo's parallel test threads would
/// race each other's `cd`s.
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn scenario_file(tag: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("termotion-load-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn loads_the_reference_brb_scenario_end_to_end() {
    let path = scenario_file(
        "brb",
        "\
version: 1
metadata:
  name: brb
canvas:
  width: 1920
  height: 1080
  fps: 30
  background: '#080B09'
theme:
  ref: zombocoder
terminal:
  x: 110
  y: 110
  width: 1700
  height: 860
prompt:
  user: zombocoder
  host: twitch
  path: '~'
  symbol: '$'
timeline:
  - type: command
    text: ./brb
    speed: 45ms
  - type: pause
    duration: 500ms
  - type: write_line
    text: Session suspended.
",
    );

    let loaded = load(&path, &Overrides::default()).unwrap();
    assert_eq!(loaded.scenario.metadata.name, "brb");
    assert_eq!(loaded.scenario.timeline.len(), 3);
    assert_eq!(loaded.scenario.prompt.user, "zombocoder");
    assert_eq!(
        loaded.scenario.palette.prompt_host,
        termotion_core::Color::rgb(0x69, 0xD2, 0xFF)
    );
}

#[test]
fn semantic_errors_carry_file_line_and_column() {
    let path = scenario_file(
        "diag",
        "version: 1\ntimeline:\n  - type: write\n    text: hi\n  - type: pause\n    duration: 0ms\n",
    );

    let errors = load(&path, &Overrides::default()).unwrap_err();
    let first = &errors[0];
    assert_eq!(
        first.code,
        termotion_schema::diag::codes::DURATION_NOT_POSITIVE
    );
    assert!(first.file.is_some(), "diagnostic must name the file");
    let pos = first.position.expect("diagnostic must carry a position");
    assert_eq!(pos.line, 6);
}

#[test]
fn variables_and_includes_compose() {
    let dir = std::env::temp_dir().join("termotion-load-compose");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("common.yaml"),
        "prompt:\n  user: zombocoder\n  host: twitch\n",
    )
    .unwrap();
    let path = dir.join("scene.yaml");
    fs::write(
        &path,
        "version: 1\nincludes:\n  - ./common.yaml\nvariables:\n  reason: coffee\ntimeline:\n  - type: write\n    text: '> {{ reason }}'\n",
    )
    .unwrap();

    let loaded = load(&path, &Overrides::default()).unwrap();
    assert_eq!(loaded.scenario.prompt.host, "twitch");
    match &loaded.scenario.timeline[0] {
        termotion_core::Action::Write { spans, .. } => assert_eq!(spans[0].text, "> coffee"),
        other => panic!("expected write, got {other:?}"),
    }
}

fn project_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("termotion-load-project-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Regression test for the bug where `ProjectConfig::load_nearest` never
/// canonicalized its start path: a relative scenario path with no directory
/// component (`demo.yaml`, the shape produced by `cd scenes && termotion render
/// demo.yaml`) made `Path::parent()` bottom out at `""` after a single step,
/// so the walk never reached the real project root and the whole `termotion.yaml`
/// defaults layer silently vanished.
///
/// This must run with the process cwd set to the scenario's directory and the
/// project root one level above it, and pass a *bare relative filename* to
/// `load` — an absolute path does not exercise the bug, because `Path::parent()`
/// on an absolute multi-component path already walks correctly without
/// canonicalization.
#[test]
fn project_config_is_found_from_a_relative_scenario_path() {
    let _guard = CWD_LOCK.lock().unwrap();
    let original_cwd = std::env::current_dir().unwrap();

    let root = project_dir("relative");
    let scenes_dir = root.join("scenes");
    fs::create_dir_all(&scenes_dir).unwrap();
    fs::write(
        root.join("termotion.yaml"),
        "defaults:\n  width: 640\n  height: 480\n  fps: 24\n  theme: terminal-amber\n",
    )
    .unwrap();
    fs::write(scenes_dir.join("demo.yaml"), "version: 1\ntimeline: []\n").unwrap();

    // Canonicalize so `scenes_dir` matches what `load_nearest` sees after its own
    // canonicalization (macOS aliases `/tmp` to `/private/tmp`).
    let scenes_dir = scenes_dir.canonicalize().unwrap();

    std::env::set_current_dir(&scenes_dir).unwrap();
    let result = load(std::path::Path::new("demo.yaml"), &Overrides::default());
    std::env::set_current_dir(&original_cwd).unwrap();

    let loaded = result.expect("project config must be found by walking up from the scenario");
    assert_eq!(loaded.scenario.canvas.size.width, 640);
    assert_eq!(loaded.scenario.canvas.size.height, 480);
    assert_eq!(
        loaded.scenario.canvas.fps,
        termotion_core::Fps::from_integer(24)
    );
}

#[test]
fn project_defaults_fill_in_values_the_scenario_omits() {
    let dir = project_dir("defaults");
    fs::write(
        dir.join("termotion.yaml"),
        "defaults:\n  width: 640\n  height: 480\n  fps: 24\n",
    )
    .unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, "version: 1\ntimeline: []\n").unwrap();

    let loaded = load(&path, &Overrides::default()).unwrap();
    assert_eq!(loaded.scenario.canvas.size.width, 640);
    assert_eq!(loaded.scenario.canvas.size.height, 480);
    assert_eq!(
        loaded.scenario.canvas.fps,
        termotion_core::Fps::from_integer(24)
    );
}

#[test]
fn a_scenario_value_beats_the_project_default() {
    let dir = project_dir("scenario-wins");
    fs::write(dir.join("termotion.yaml"), "defaults:\n  width: 640\n").unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, "version: 1\ncanvas:\n  width: 1280\n").unwrap();

    let loaded = load(&path, &Overrides::default()).unwrap();
    assert_eq!(loaded.scenario.canvas.size.width, 1280);
}

#[test]
fn a_cli_override_beats_both_the_scenario_and_the_project() {
    let dir = project_dir("cli-wins");
    fs::write(dir.join("termotion.yaml"), "defaults:\n  width: 640\n").unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, "version: 1\ncanvas:\n  width: 1280\n").unwrap();

    let overrides = Overrides {
        width: Some(3840),
        ..Overrides::default()
    };
    let loaded = load(&path, &overrides).unwrap();
    assert_eq!(loaded.scenario.canvas.size.width, 3840);
}

#[test]
fn a_malformed_project_file_is_ignored_rather_than_fatal() {
    let dir = project_dir("malformed");
    // `width` must be a u32; this fails to deserialize.
    fs::write(
        dir.join("termotion.yaml"),
        "defaults:\n  width: not-a-number\n",
    )
    .unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, "version: 1\ntimeline: []\n").unwrap();

    let loaded = load(&path, &Overrides::default())
        .expect("a broken termotion.yaml must not block a render");
    assert_eq!(loaded.scenario.canvas.size.width, 1920);
    assert_eq!(loaded.scenario.canvas.size.height, 1080);
}
