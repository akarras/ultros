use maud::{Render, html};
use thousands::Separable;
pub(crate) struct Gil(pub i32);


impl Render for Gil {
    fn render(&self) -> maud::Markup {
        html! {
            span class="gil" {
                ((self.0.separate_with_commas()))
            }
        }
    }
}
