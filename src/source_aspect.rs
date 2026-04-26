use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Document, Object};

use crate::config::BBoxMode;

#[derive(Debug, Clone, Copy)]
struct BoxSize {
    width: f64,
    height: f64,
}

impl BoxSize {
    fn new(width: f64, height: f64) -> Result<Self, String> {
        if width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite() {
            Ok(Self { width, height })
        } else {
            Err(format!("invalid bbox dimensions {width} x {height}"))
        }
    }

    fn aspect(self) -> f64 {
        self.width / self.height
    }
}

pub fn resolve_source_path(base_dir: &Path, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

pub fn derive_aspect(base_dir: &Path, source: &str, bbox: BBoxMode) -> Result<f64, String> {
    let path = resolve_source_path(base_dir, source);
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("svg") => derive_svg_aspect(&path, bbox),
        Some(ext) if ext.eq_ignore_ascii_case("pdf") => derive_pdf_aspect(&path, bbox),
        Some(ext) => Err(format!("unsupported source extension .{ext}")),
        None => Err("source has no file extension".into()),
    }
}

fn derive_svg_aspect(path: &Path, bbox: BBoxMode) -> Result<f64, String> {
    match bbox {
        BBoxMode::Media => derive_svg_media_aspect(path),
        BBoxMode::Crop => {
            Err("SVG has no crop box; use bbox = \"media\", bbox = \"tight\", or specify aspect explicitly".into())
        }
        BBoxMode::Tight => derive_svg_tight_aspect(path),
    }
}

fn derive_svg_media_aspect(path: &Path) -> Result<f64, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if let Some(view_box) = attr_value(&text, "viewBox").or_else(|| attr_value(&text, "viewbox")) {
        let nums: Vec<f64> = view_box
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
            return Ok(nums[2] / nums[3]);
        }
    }
    let width = attr_value(&text, "width").and_then(|s| parse_svg_length(&s));
    let height = attr_value(&text, "height").and_then(|s| parse_svg_length(&s));
    match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Ok(w / h),
        _ => Err("SVG must have viewBox or width/height metadata".into()),
    }
}

fn derive_svg_tight_aspect(path: &Path) -> Result<f64, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &options).map_err(|e| e.to_string())?;
    let bbox = svg_tight_bbox(tree.root())?;
    BoxSize::new(bbox.width() as f64, bbox.height() as f64).map(BoxSize::aspect)
}

fn svg_tight_bbox(root: &usvg::Group) -> Result<usvg::Rect, String> {
    let mut bbox: Option<usvg::Rect> = None;
    collect_svg_bbox(root, &mut bbox);
    bbox.ok_or_else(|| {
        "SVG tight bbox could not be computed because no visible content was found".into()
    })
}

fn collect_svg_bbox(group: &usvg::Group, bbox: &mut Option<usvg::Rect>) {
    for node in group.children() {
        let node_box = node.abs_stroke_bounding_box();
        if !node_box.is_empty() {
            *bbox = match bbox {
                Some(existing) => existing.join(&node_box).or(Some(*existing)),
                None => Some(node_box),
            };
        }
        if let usvg::Node::Group(child) = node {
            collect_svg_bbox(child, bbox);
        }
        node.subroots(|subroot| collect_svg_bbox(subroot, bbox));
    }
}

fn derive_pdf_aspect(path: &Path, bbox: BBoxMode) -> Result<f64, String> {
    match bbox {
        BBoxMode::Media => pdf_box_aspect(path, PdfBoxKind::Media),
        BBoxMode::Crop => pdf_box_aspect(path, PdfBoxKind::Crop),
        BBoxMode::Tight => derive_pdf_tight_aspect(path),
    }
}

#[derive(Debug, Clone, Copy)]
enum PdfBoxKind {
    Media,
    Crop,
}

fn pdf_box_aspect(path: &Path, kind: PdfBoxKind) -> Result<f64, String> {
    let doc = Document::load(path).map_err(|e| e.to_string())?;
    let pages = doc.get_pages();
    let Some((_page_number, page_id)) = pages.iter().next() else {
        return Err("PDF has no pages".into());
    };
    let page = doc
        .get_object(*page_id)
        .map_err(|e| e.to_string())?
        .as_dict()
        .map_err(|e| e.to_string())?;
    let rect = match kind {
        PdfBoxKind::Media => pdf_rect_from_dict(&doc, page, b"MediaBox")?
            .ok_or_else(|| "first PDF page has no MediaBox".to_string())?,
        PdfBoxKind::Crop => pdf_rect_from_dict(&doc, page, b"CropBox")?
            .or(pdf_rect_from_dict(&doc, page, b"MediaBox")?)
            .ok_or_else(|| "first PDF page has neither CropBox nor MediaBox".to_string())?,
    };
    rect.map(BoxSize::aspect)
}

fn pdf_rect_from_dict(
    doc: &Document,
    dict: &lopdf::Dictionary,
    key: &[u8],
) -> Result<Option<Result<BoxSize, String>>, String> {
    let Ok(obj) = dict.get_deref(key, doc) else {
        return Ok(None);
    };
    let arr = obj.as_array().map_err(|e| e.to_string())?;
    if arr.len() != 4 {
        return Ok(Some(Err(format!(
            "{} must contain exactly 4 numbers",
            String::from_utf8_lossy(key)
        ))));
    }
    let left = pdf_number(&arr[0])?;
    let bottom = pdf_number(&arr[1])?;
    let right = pdf_number(&arr[2])?;
    let top = pdf_number(&arr[3])?;
    Ok(Some(BoxSize::new(
        (right - left).abs(),
        (top - bottom).abs(),
    )))
}

fn pdf_number(obj: &Object) -> Result<f64, String> {
    obj.as_float()
        .map(f64::from)
        .map_err(|e| format!("expected numeric PDF box coordinate: {e}"))
}

fn derive_pdf_tight_aspect(path: &Path) -> Result<f64, String> {
    let tempdir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let svg_path = tempdir.path().join("page.svg");
    render_pdf_tight_svg_with_cmd("mutool", path, &svg_path)?;
    derive_svg_tight_aspect(&svg_path)
}

fn render_pdf_tight_svg_with_cmd(
    cmd: &str,
    pdf_path: &Path,
    svg_path: &Path,
) -> Result<(), String> {
    let output = Command::new(cmd)
        .args(["draw", "-q", "-F", "svg", "-o"])
        .arg(svg_path)
        .arg(pdf_path)
        .arg("1")
        .output()
        .map_err(|e| {
            format!(
                "failed to run {cmd} for PDF tight bbox: {e}; install MuPDF/mutool or specify aspect explicitly"
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "{cmd} failed while deriving PDF tight bbox; install MuPDF/mutool or specify aspect explicitly\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !svg_path.exists() {
        return Err(format!(
            "{cmd} did not produce an SVG for PDF tight bbox derivation"
        ));
    }
    Ok(())
}

fn attr_value(text: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let start = text.find(&needle)? + needle.len();
    let quote = text[start..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &text[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn parse_svg_length(raw: &str) -> Option<f64> {
    let number: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    number.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, Object, Stream, dictionary};

    fn write_pdf(path: &Path, media: [i32; 4], crop: Option<[i32; 4]>, content: &[u8]) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
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

    #[test]
    fn svg_media_uses_viewbox() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 100"></svg>"#,
        )
        .unwrap();
        assert_eq!(
            derive_aspect(tmp.path(), "panel.svg", BBoxMode::Media).unwrap(),
            3.0
        );
    }

    #[test]
    fn svg_media_uses_width_height_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="240pt" height="120pt"></svg>"#,
        )
        .unwrap();
        assert_eq!(
            derive_aspect(tmp.path(), "panel.svg", BBoxMode::Media).unwrap(),
            2.0
        );
    }

    #[test]
    fn svg_tight_uses_visible_content_bounds() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 100">
  <rect x="40" y="10" width="80" height="40" fill="black"/>
</svg>"#,
        )
        .unwrap();
        assert!(
            (derive_aspect(tmp.path(), "panel.svg", BBoxMode::Tight).unwrap() - 2.0).abs() < 1e-6
        );
    }

    #[test]
    fn svg_crop_fails_actionably() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#,
        )
        .unwrap();
        let err = derive_aspect(tmp.path(), "panel.svg", BBoxMode::Crop).unwrap_err();
        assert!(err.contains("SVG has no crop box"));
        assert!(err.contains("specify aspect explicitly"));
    }

    #[test]
    fn pdf_media_and_crop_read_first_page_boxes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.pdf");
        write_pdf(
            &path,
            [0, 0, 400, 200],
            Some([0, 0, 300, 100]),
            b"0 0 300 100 re f",
        );
        assert_eq!(
            derive_aspect(tmp.path(), "panel.pdf", BBoxMode::Media).unwrap(),
            2.0
        );
        assert_eq!(
            derive_aspect(tmp.path(), "panel.pdf", BBoxMode::Crop).unwrap(),
            3.0
        );
    }

    #[test]
    fn pdf_crop_falls_back_to_media_when_cropbox_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.pdf");
        write_pdf(&path, [0, 0, 500, 250], None, b"0 0 500 250 re f");
        assert_eq!(
            derive_aspect(tmp.path(), "panel.pdf", BBoxMode::Crop).unwrap(),
            2.0
        );
    }

    #[test]
    fn pdf_tight_uses_mutool_svg_conversion_when_available() {
        if Command::new("mutool").arg("-h").output().is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("panel.pdf");
        write_pdf(&path, [0, 0, 400, 400], None, b"0 0 200 100 re f");
        let aspect = derive_aspect(tmp.path(), "panel.pdf", BBoxMode::Tight).unwrap();
        assert!((aspect - 2.0).abs() < 0.05, "aspect = {aspect}");
    }

    #[test]
    fn pdf_tight_missing_mutool_fails_actionably() {
        let tmp = tempfile::tempdir().unwrap();
        let pdf_path = tmp.path().join("panel.pdf");
        let svg_path = tmp.path().join("panel.svg");
        write_pdf(&pdf_path, [0, 0, 100, 100], None, b"0 0 100 100 re f");

        let err = render_pdf_tight_svg_with_cmd(
            "figurefit-mutool-that-does-not-exist",
            &pdf_path,
            &svg_path,
        )
        .unwrap_err();
        assert!(err.contains("install MuPDF/mutool"));
        assert!(err.contains("specify aspect explicitly"));
    }
}
