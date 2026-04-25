//! Emit lightweight SVG previews for FigureFit layouts.

use crate::tree::SolvedLayout;
use std::fmt::Write;

pub fn emit_preview_svg(layout: &SolvedLayout) -> String {
    let width = layout.page_width_mm;
    let height = layout.height_mm.max(1.0);
    let mut out = String::new();
    writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.1} {height:.1}" width="{width:.1}mm" height="{height:.1}mm">"#
    )
    .unwrap();
    writeln!(out, r#"<rect width="100%" height="100%" fill="white"/>"#).unwrap();

    for p in &layout.placements {
        let x = p.rect.x * width;
        let y = p.rect.y * height;
        let w = p.rect.w * width;
        let h = p.rect.h * height;
        let fill = if p.void_frac > 0.15 {
            "#fff3cd"
        } else {
            "#edf7ff"
        };
        writeln!(
            out,
            r##"<rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" fill="{fill}" stroke="#335c81" stroke-width="0.5"/>"##
        )
        .unwrap();
        writeln!(
            out,
            r##"<text x="{:.2}" y="{:.2}" font-family="sans-serif" font-size="5" fill="#111">{} {:.1}x{:.1}mm</text>"##,
            x + 1.5,
            y + 5.5,
            xml_escape(&p.panel_id),
            p.w_mm,
            p.h_mm
        )
        .unwrap();
        if p.void_frac > 0.01 {
            writeln!(
                out,
                r##"<text x="{:.2}" y="{:.2}" font-family="sans-serif" font-size="3.5" fill="#8a5a00">void {:.0}%</text>"##,
                x + 1.5,
                y + 10.0,
                p.void_frac * 100.0
            )
            .unwrap();
        }
    }

    for d in &layout.dropped_groups {
        writeln!(
            out,
            r##"<text x="2" y="{:.2}" font-family="sans-serif" font-size="4" fill="#a01818">dropped {}: {}</text>"##,
            height - 4.0,
            xml_escape(&d.group_id),
            xml_escape(&d.reason)
        )
        .unwrap();
    }

    writeln!(out, "</svg>").unwrap();
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
