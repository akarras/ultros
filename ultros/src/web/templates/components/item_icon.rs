use maud::{html, Render};

pub(crate) struct ItemIcon {
    pub(crate) item_id: i32,
    pub(crate) icon_size: IconSize,
}

pub(crate) enum IconSize {
    Small,
    Medium,
    Large,
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

impl Render for ItemIcon {
    fn render(&self) -> maud::Markup {
        html! {
          img class=((self.icon_size.get_class())) src={"/static/itemicon/" ((self.item_id)) "?size="((self.icon_size.get_px_size()))} {

          }
        }
    }
}
