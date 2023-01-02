use leptos::*;

pub enum IconSize {
    Small,
    Medium,
    Large
}


impl IconSize {
    fn get_class(&self) -> &str {
        match self {
            IconSize::Small => "icon-small",
            IconSize::Medium => "icon-medium",
            IconSize::Large => "icon-large",
        }
    }

    fn get_px_size(&self) -> i32 {
        match self {
            IconSize::Small => 30,
            IconSize::Medium => 40,
            IconSize::Large => 80,
        }
    }
}

#[component]
pub fn ItemIcon(cx: Scope, item_id: i32, icon_size: IconSize) -> impl IntoView {
    view! {
        cx,
        <img class={icon_size.get_class()} src={format!("/static/itemicon/{item_id}?size={}", icon_size.get_px_size())} />
    }
}
