//! Serialize a [`Scene`] to an SVG string.
//!
//! The server rasterizes this with resvg, so stick to plain SVG 1.1 that
//! usvg supports: no CSS classes, no `rgba()` colors (use `*-opacity`
//! attributes), `xlink:href` for images.
//!
//! **Attribute escaping:** attribute values (`font_family`, `Image::href`) are
//! not escaped — callers must supply attribute-safe values (today they're
//! internal constants / data URIs).

use std::fmt::Write;

use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};

fn hex(c: &Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn push_fill(out: &mut String, c: &Color) {
    let _ = write!(out, r#" fill="{}""#, hex(c));
    if c.a < 1.0 {
        let _ = write!(out, r#" fill-opacity="{:.3}""#, c.a);
    }
}

fn push_stroke(out: &mut String, s: &Stroke) {
    let _ = write!(
        out,
        r#" stroke="{}" stroke-width="{:.2}" stroke-linecap="round" stroke-linejoin="round""#,
        hex(&s.color),
        s.width
    );
    if s.color.a < 1.0 {
        let _ = write!(out, r#" stroke-opacity="{:.3}""#, s.color.a);
    }
    if let Some((dash, gap)) = s.dash {
        let _ = write!(out, r#" stroke-dasharray="{dash:.1} {gap:.1}""#);
    }
}

fn points_attr(points: &[(f32, f32)]) -> String {
    let mut out = String::with_capacity(points.len() * 12);
    for (i, (x, y)) in points.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{x:.1},{y:.1}");
    }
    out
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn scene_to_svg(scene: &Scene) -> String {
    let mut out = String::with_capacity(scene.nodes.len() * 96 + 256);
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="{font}">"#,
        w = scene.width,
        h = scene.height,
        font = escape_text(&scene.font_family),
    );
    if let Some(bg) = &scene.background {
        let _ = write!(
            out,
            r#"<rect x="0" y="0" width="{:.0}" height="{:.0}""#,
            scene.width, scene.height
        );
        push_fill(&mut out, bg);
        out.push_str("/>");
    }
    for node in &scene.nodes {
        match node {
            Node::Rect {
                x,
                y,
                width,
                height,
                rx,
                fill,
            } => {
                let _ = write!(
                    out,
                    r#"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}""#
                );
                if *rx > 0.0 {
                    let _ = write!(out, r#" rx="{rx:.1}""#);
                }
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
            Node::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
            } => {
                let _ = write!(
                    out,
                    r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}""#
                );
                push_stroke(&mut out, stroke);
                out.push_str("/>");
            }
            Node::Polyline { points, stroke } => {
                let _ = write!(
                    out,
                    r#"<polyline points="{}" fill="none""#,
                    points_attr(points)
                );
                push_stroke(&mut out, stroke);
                out.push_str("/>");
            }
            Node::Area {
                points,
                baseline_y,
                fill,
            } => {
                if points.len() < 2 {
                    continue;
                }
                let mut d = String::new();
                for (i, (x, y)) in points.iter().enumerate() {
                    let _ = write!(d, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" });
                }
                let first_x = points[0].0;
                let last_x = points[points.len() - 1].0;
                let _ = write!(
                    d,
                    "L{last_x:.1} {baseline_y:.1}L{first_x:.1} {baseline_y:.1}Z"
                );
                let _ = write!(out, r#"<path d="{d}""#);
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
            Node::Circle { cx, cy, r, fill } => {
                let _ = write!(out, r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}""#);
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
            Node::Text {
                x,
                y,
                content,
                size,
                color,
                anchor,
                bold,
            } => {
                let anchor = match anchor {
                    TextAnchor::Start => "start",
                    TextAnchor::Middle => "middle",
                    TextAnchor::End => "end",
                };
                let _ = write!(
                    out,
                    r#"<text x="{x:.1}" y="{y:.1}" font-size="{size:.1}" text-anchor="{anchor}""#
                );
                if *bold {
                    out.push_str(r#" font-weight="bold""#);
                }
                push_fill(&mut out, color);
                let _ = write!(out, ">{}</text>", escape_text(content));
            }
            Node::Image {
                x,
                y,
                width,
                height,
                href,
            } => {
                let _ = write!(
                    out,
                    r#"<image x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}" xlink:href="{href}"/>"#
                );
            }
        }
    }
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};

    #[test]
    fn serializes_each_node_kind() {
        let scene = Scene {
            width: 100.0,
            height: 50.0,
            background: Some(Color::hex("#202124")),
            font_family: "Jaldi, sans-serif".to_string(),
            nodes: vec![
                Node::Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 3.0,
                    height: 4.0,
                    rx: 1.0,
                    fill: Color::rgb(255, 0, 0).with_alpha(0.5),
                },
                Node::Line {
                    x1: 0.0,
                    y1: 0.0,
                    x2: 10.0,
                    y2: 10.0,
                    stroke: Stroke {
                        color: Color::rgb(0, 255, 0),
                        width: 1.5,
                        dash: Some((2.0, 4.0)),
                    },
                },
                Node::Polyline {
                    points: vec![(0.0, 0.0), (5.0, 5.0)],
                    stroke: Stroke {
                        color: Color::rgb(0, 0, 255),
                        width: 2.0,
                        dash: None,
                    },
                },
                Node::Area {
                    points: vec![(0.0, 10.0), (5.0, 5.0)],
                    baseline_y: 20.0,
                    fill: Color::rgb(1, 2, 3),
                },
                Node::Circle {
                    cx: 5.0,
                    cy: 5.0,
                    r: 2.0,
                    fill: Color::rgb(9, 9, 9),
                },
                Node::Text {
                    x: 1.0,
                    y: 1.0,
                    content: "a < b".to_string(),
                    size: 13.0,
                    color: Color::rgb(0, 0, 0),
                    anchor: TextAnchor::Middle,
                    bold: true,
                },
                Node::Image {
                    x: 0.0,
                    y: 0.0,
                    width: 8.0,
                    height: 8.0,
                    href: "data:image/png;base64,AAAA".to_string(),
                },
            ],
        };
        let svg = scene_to_svg(&scene);
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains(r#"viewBox="0 0 100 50""#));
        assert!(svg.contains(r##"fill="#202124""##)); // background rect
        assert!(svg.contains(r##"fill="#ff0000" fill-opacity="0.500""##));
        assert!(svg.contains(r#"stroke-dasharray="2.0 4.0""#));
        assert!(svg.contains(r##"<polyline points="0.0,0.0 5.0,5.0" fill="none""##));
        assert!(svg.contains(r##"Z" fill="#010203""##)); // area path closes to baseline
        assert!(svg.contains(r#"<circle cx="5.0" cy="5.0" r="2.0""#));
        assert!(svg.contains("a &lt; b"));
        assert!(svg.contains(r#"text-anchor="middle""#));
        assert!(svg.contains(r#"font-weight="bold""#));
        assert!(svg.contains(r#"xlink:href="data:image/png;base64,AAAA""#));
    }

    #[test]
    fn empty_scene_is_well_formed() {
        let scene = Scene {
            width: 10.0,
            height: 10.0,
            background: None,
            font_family: "sans-serif".to_string(),
            nodes: Vec::new(),
        };
        let svg = scene_to_svg(&scene);
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
    }
}
