//! Golden frame tests.
//!
//! Regenerate after an intentional rendering change:
//!     TERMOTION_UPDATE_GOLDEN=1 cargo test -p termotion-cli --test golden
//! Review the resulting image diff before committing — a golden that captures a
//! bug becomes the permanent reference for correct output.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn golden_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// Renders a fixture to PNGs and compares the requested frames against goldens.
fn check(scene: &str, frames: &[u32]) {
    let name = scene.trim_end_matches(".yaml");
    let out = std::env::temp_dir().join(format!("termotion-golden-{name}"));
    let _ = fs::remove_dir_all(&out);

    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            fixture(scene).to_str().unwrap(),
            "--format",
            "png",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let update = std::env::var("TERMOTION_UPDATE_GOLDEN").is_ok();
    let dir = golden_dir(name);
    if update {
        fs::create_dir_all(&dir).unwrap();
    }

    for frame in frames {
        let rendered = out.join(format!("{frame:06}.png"));
        let expected = dir.join(format!("{frame:06}.png"));
        let actual = fs::read(&rendered)
            .unwrap_or_else(|_| panic!("frame {frame} was not rendered for {scene}"));

        if update {
            fs::write(&expected, &actual).unwrap();
            continue;
        }

        let golden = fs::read(&expected).unwrap_or_else(|_| {
            panic!(
                "missing golden {}; run with TERMOTION_UPDATE_GOLDEN=1",
                expected.display()
            )
        });
        assert_eq!(
            actual, golden,
            "frame {frame} of {scene} differs from its golden image"
        );
    }
}

#[test]
fn golden_basic_write() {
    check("basic-write.yaml", &[1, 6, 11]);
}

#[test]
fn golden_cursor() {
    // Frame 1 is blink-on, frame 6 is blink-off at 10fps with a 500ms period.
    check("cursor.yaml", &[1, 6]);
}

#[test]
fn golden_prompt() {
    check("prompt.yaml", &[1, 5]);
}

#[test]
fn golden_unicode() {
    check("unicode.yaml", &[1]);
}

#[test]
fn golden_scroll() {
    // `scroll.yaml` types 10 numbered lines into a 6-row terminal under the
    // default `overflow: scroll` mode, so this is the only fixture that ever
    // exercises `Grid::scroll_up`. Frame 8 (t=700ms) is the moment "line 01"
    // has just landed in row 0 with nothing pushed off yet — a pre-scroll
    // reference frame. Frame 70, the final frame, is well after several scrolls:
    // line 10 is fully typed but its trailing newline (at 7000ms) falls just
    // past the last frame's timestamp (6900ms), so the frame shows the tail of
    // the scrolled-past content ("line 05".."line 10") settled in place, making
    // it obvious at a glance that rows shifted rather than stacked forever.
    check("scroll.yaml", &[8, 70]);
}
