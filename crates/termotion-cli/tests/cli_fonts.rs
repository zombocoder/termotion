use assert_cmd::Command;

#[test]
fn fonts_lists_the_embedded_face() {
    Command::cargo_bin("termotion")
        .unwrap()
        .arg("fonts")
        .assert()
        .success()
        .stdout(predicates::str::contains("JetBrains Mono"));
}

#[test]
fn fonts_reports_what_a_scenario_resolves_to() {
    let dir = std::env::temp_dir().join("termotion-fonts-scene");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("scene.yaml");
    std::fs::write(&scene, "version: 1\nfont:\n  size: 24\ntimeline: []\n").unwrap();

    Command::cargo_bin("termotion")
        .unwrap()
        .args(["fonts", "--scenario", scene.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("advance"))
        .stdout(predicates::str::contains("JetBrains Mono"));
}

#[test]
fn completions_emit_a_script_for_each_shell() {
    for shell in ["bash", "zsh", "fish"] {
        let output = Command::cargo_bin("termotion")
            .unwrap()
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell} completions failed");
        assert!(!output.stdout.is_empty(), "{shell} completions were empty");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("termotion"),
            "{shell} completions should mention the binary"
        );
    }
}
