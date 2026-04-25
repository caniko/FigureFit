use std::process::Command;

fn write_spec(dir: &std::path::Path) -> std::path::PathBuf {
    let spec = dir.join("CLITest.toml");
    std::fs::write(
        &spec,
        r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "CLITest"

[[panel]]
id = "A"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#,
    )
    .unwrap();
    spec
}

#[test]
fn figurefit_cli_writes_report_preview_and_passes_check() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = write_spec(tmp.path());
    let output = Command::new(env!("CARGO_BIN_EXE_figurefit"))
        .arg(&spec)
        .arg("--preview")
        .arg("--explain")
        .arg("--top-n")
        .arg("1")
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(tmp.path().join("CLITest_layout.tex").exists());
    let report_path = tmp.path().join("CLITest_report.toml");
    let preview_path = tmp.path().join("CLITest_preview.svg");
    assert!(report_path.exists());
    assert!(preview_path.exists());

    let report_text = std::fs::read_to_string(report_path).unwrap();
    let report: toml::Value = toml::from_str(&report_text).unwrap();
    assert_eq!(report["name"].as_str(), Some("CLITest"));
    assert_eq!(report["checks"]["passed"].as_bool(), Some(true));
    assert_eq!(report["candidate"].as_array().map(Vec::len), Some(1));

    let preview_text = std::fs::read_to_string(preview_path).unwrap();
    assert!(preview_text.contains("<svg"));
    assert!(preview_text.contains(">A "));
}

#[test]
fn deprecated_alias_still_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = write_spec(tmp.path());
    let output = Command::new(env!("CARGO_BIN_EXE_figure-layout"))
        .arg(&spec)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("deprecated"));
}

#[test]
fn check_flag_fails_on_quality_gate_and_reports_issue() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = tmp.path().join("FailCheck.toml");
    std::fs::write(
        &spec,
        r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "FailCheck"

[quality]
min_height_utilization = 0.99

[[panel]]
id = "A"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_figurefit"))
        .arg(&spec)
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("height utilization"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_derives_svg_source_aspect_and_rejects_deferred_bbox_modes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("panel.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 100"></svg>"#,
    )
    .unwrap();
    let spec = tmp.path().join("SourceAspect.toml");
    std::fs::write(
        &spec,
        r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "SourceAspect"

[[panel]]
id = "A"
source = "panel.svg"

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_figurefit"))
        .arg(&spec)
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = std::fs::read_to_string(tmp.path().join("SourceAspect_report.toml")).unwrap();
    let report: toml::Value = toml::from_str(&report_text).unwrap();
    assert_eq!(report["placed"][0]["aspect_target"].as_float(), Some(3.0));

    let deferred = tmp.path().join("DeferredBBox.toml");
    std::fs::write(
        &deferred,
        r#"
[page]
width_mm = 160
max_height_mm = 216

[[panel]]
id = "A"
source = "panel.svg"
bbox = "tight"

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_figurefit"))
        .arg(&deferred)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not yet supported"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
