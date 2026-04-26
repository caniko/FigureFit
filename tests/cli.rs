use std::process::Command;

use lopdf::{Document, Object, Stream, dictionary};

fn write_pdf(path: &std::path::Path, media: [i32; 4], crop: Option<[i32; 4]>) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"0 0 120 60 re f".to_vec()));
    let mut page = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => media.iter().map(|n| Object::Integer(i64::from(*n))).collect::<Vec<_>>(),
    };
    if let Some(crop) = crop {
        page.set(
            "CropBox",
            crop.iter()
                .map(|n| Object::Integer(i64::from(*n)))
                .collect::<Vec<_>>(),
        );
    }
    let page_id = doc.add_object(page);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

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
fn cli_derives_svg_media_and_tight_source_aspects() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("panel.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 100">
  <rect x="0" y="0" width="80" height="40" fill="black"/>
</svg>"#,
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

    let tight = tmp.path().join("TightBBox.toml");
    std::fs::write(
        &tight,
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
        .arg(&tight)
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report_text = std::fs::read_to_string(tmp.path().join("TightBBox_report.toml")).unwrap();
    let report: toml::Value = toml::from_str(&report_text).unwrap();
    assert_eq!(report["placed"][0]["aspect_target"].as_float(), Some(2.0));
}

#[test]
fn cli_derives_pdf_media_and_crop_source_aspects() {
    let tmp = tempfile::tempdir().unwrap();
    write_pdf(
        &tmp.path().join("panel.pdf"),
        [0, 0, 400, 200],
        Some([0, 0, 300, 100]),
    );
    let spec = tmp.path().join("PdfCrop.toml");
    std::fs::write(
        &spec,
        r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "PdfCrop"

[[panel]]
id = "A"
source = "panel.pdf"
bbox = "crop"

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
    let report_text = std::fs::read_to_string(tmp.path().join("PdfCrop_report.toml")).unwrap();
    let report: toml::Value = toml::from_str(&report_text).unwrap();
    assert_eq!(report["placed"][0]["aspect_target"].as_float(), Some(3.0));
}

#[test]
fn cli_rejects_unsupported_svg_crop() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("panel.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 100"></svg>"#,
    )
    .unwrap();
    let spec = tmp.path().join("SvgCrop.toml");
    std::fs::write(
        &spec,
        r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "SvgCrop"

[[panel]]
id = "A"
source = "panel.svg"
bbox = "crop"

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_figurefit"))
        .arg(&spec)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("SVG has no crop box"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
