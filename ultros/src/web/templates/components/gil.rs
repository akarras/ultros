use maud::{html, Render};
use thousands::Separable;
pub(crate) struct Gil(pub i32);

impl Render for Gil {
    fn render(&self) -> maud::Markup {
        html! {
            span class="gil" {
                img src="/static/images/gil.webp";
                ((self.0.separate_with_commas()))
            }
        }
    }
}
