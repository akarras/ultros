//! Leptos renderer over [`Scene`] — the browser counterpart of [`crate::svg`].
//!
//! [`scene_view`] maps display-list nodes to SVG view nodes. It renders a
//! snapshot of the scene; reactivity comes from the caller rebuilding the
//! scene inside a memo/closure. Hover layers are drawn by the app on top.

use leptos::prelude::*;

use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};
use crate::svg::{area_path_d, points_attr};

/// CSS color string. Browsers accept `rgba()`, so unlike the resvg-bound
/// serializer this needs no separate `*-opacity` attributes.
pub fn color_attr(c: &Color) -> String {
    if c.a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a)
    }
}

fn px(v: f32) -> String {
    format!("{v:.1}")
}

fn dash_attr(stroke: &Stroke) -> Option<String> {
    stroke.dash.map(|(dash, gap)| format!("{dash:.1} {gap:.1}"))
}

/// Render the scene's nodes (plus its background) as SVG children. Embed
/// inside an `<svg viewBox="0 0 {scene.width} {scene.height}">`.
pub fn scene_view(scene: &Scene) -> impl IntoView {
    let background = scene.background.as_ref().map(|bg| {
        view! {
            <rect x="0" y="0" width=px(scene.width) height=px(scene.height) fill=color_attr(bg) />
        }
    });
    let nodes = scene.nodes.iter().map(node_view).collect_view();
    view! {
        {background}
        <g font-family=scene.font_family.clone()>{nodes}</g>
    }
}

fn node_view(node: &Node) -> AnyView {
    match node {
        Node::Rect {
            x,
            y,
            width,
            height,
            rx,
            fill,
        } => view! {
            <rect
                x=px(*x)
                y=px(*y)
                width=px(*width)
                height=px(*height)
                rx=(*rx > 0.0).then(|| px(*rx))
                fill=color_attr(fill)
            />
        }
        .into_any(),
        Node::Line {
            x1,
            y1,
            x2,
            y2,
            stroke,
        } => view! {
            <line
                x1=px(*x1)
                y1=px(*y1)
                x2=px(*x2)
                y2=px(*y2)
                stroke=color_attr(&stroke.color)
                stroke-width=px(stroke.width)
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-dasharray=dash_attr(stroke)
            />
        }
        .into_any(),
        Node::Polyline { points, stroke } => view! {
            <polyline
                points=points_attr(points)
                fill="none"
                stroke=color_attr(&stroke.color)
                stroke-width=px(stroke.width)
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-dasharray=dash_attr(stroke)
            />
        }
        .into_any(),
        Node::Area {
            points,
            baseline_y,
            fill,
        } => match area_path_d(points, *baseline_y) {
            Some(d) => view! { <path d=d fill=color_attr(fill) /> }.into_any(),
            None => ().into_any(),
        },
        Node::Circle { cx, cy, r, fill } => view! {
            <circle cx=px(*cx) cy=px(*cy) r=px(*r) fill=color_attr(fill) />
        }
        .into_any(),
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
            view! {
                <text
                    x=px(*x)
                    y=px(*y)
                    font-size=px(*size)
                    text-anchor=anchor
                    font-weight=bold.then_some("bold")
                    fill=color_attr(color)
                >
                    {content.clone()}
                </text>
            }
            .into_any()
        }
        Node::Image {
            x,
            y,
            width,
            height,
            href,
        } => view! {
            <image x=px(*x) y=px(*y) width=px(*width) height=px(*height) href=href.clone() />
        }
        .into_any(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};

    #[test]
    fn renders_scene_nodes_as_svg_markup() {
        let scene = Scene {
            width: 100.0,
            height: 50.0,
            background: Some(Color::hex("#202124")),
            font_family: "sans-serif".to_string(),
            nodes: vec![
                Node::Circle {
                    cx: 5.0,
                    cy: 5.0,
                    r: 2.0,
                    fill: Color::rgb(9, 9, 9).with_alpha(0.5),
                },
                Node::Polyline {
                    points: vec![(0.0, 0.0), (5.0, 5.0)],
                    stroke: Stroke {
                        color: Color::rgb(0, 0, 255),
                        width: 2.0,
                        dash: Some((2.0, 4.0)),
                    },
                },
                Node::Area {
                    points: vec![(0.0, 10.0), (5.0, 5.0)],
                    baseline_y: 20.0,
                    fill: Color::rgb(1, 2, 3),
                },
                Node::Text {
                    x: 1.0,
                    y: 1.0,
                    content: "hi".to_string(),
                    size: 13.0,
                    color: Color::rgb(0, 0, 0),
                    anchor: TextAnchor::Middle,
                    bold: true,
                },
            ],
        };
        let html = scene_view(&scene).to_html();
        assert!(html.contains("<rect"), "background rect: {html}");
        assert!(html.contains("rgba(9,9,9,0.500)"));
        assert!(html.contains("<polyline"));
        assert!(html.contains("stroke-dasharray=\"2.0 4.0\""));
        assert!(html.contains("<path"));
        assert!(html.contains("text-anchor=\"middle\""));
        assert!(html.contains(">hi</text>"));
    }
}
