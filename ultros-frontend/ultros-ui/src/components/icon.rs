use leptos::prelude::*;
use leptos::svg;

#[component]
pub fn Icon(
    /// The icon to render.
    #[prop(into)]
    icon: Signal<icondata_core::Icon>,
    #[prop(into, optional)] style: MaybeProp<String>,
    #[prop(into, optional)] width: MaybeProp<String>,
    #[prop(into, optional)] height: MaybeProp<String>,
    #[prop(into, optional)] aria_hidden: MaybeProp<bool>,
) -> impl IntoView {
    move || {
        let icon = icon.get();

        // Wrap the icon data in a <g> to ensure InertElement always gets a single top
        // level element.
        let mut data = String::with_capacity(icon.data.len() + 7);
        data.push_str("<g>");
        data.push_str(icon.data);
        data.push_str("</g>");

        svg::svg()
            .style(match (style.get(), icon.style) {
                (Some(a), Some(b)) => Some(format!("{b} {a}")),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b.to_string()),
                _ => None,
            })
            .attr("x", icon.x)
            .attr("y", icon.y)
            .attr("width", width.get().unwrap_or_else(|| "1em".to_string()))
            .attr("height", height.get().unwrap_or_else(|| "1em".to_string()))
            .attr("viewBox", icon.view_box)
            .attr("stroke-linecap", icon.stroke_linecap)
            .attr("stroke-linejoin", icon.stroke_linejoin)
            .attr("stroke-width", icon.stroke_width)
            .attr("stroke", icon.stroke)
            .attr("fill", icon.fill.unwrap_or("currentColor"))
            .attr("role", "graphics-symbol")
            .attr("aria-hidden", move || {
                if aria_hidden.get().unwrap_or(false) {
                    Some("true")
                } else {
                    None
                }
            })
            .child(svg::InertElement::new(data))
    }
}
