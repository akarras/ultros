use maud::{html, Render};

pub(crate) struct CopyTextButton<'a> {
    pub(crate) text: &'a str,
}

impl<'a> Render for CopyTextButton<'a> {
    fn render(&self) -> maud::Markup {
        html! {
            div class="tooltip" {
                i class="fa-regular fa-clipboard clipboard" onclick={"navigator.clipboard.writeText(\"" ((self.text)) "\"); this.nextSibling.innerHTML = \"Copied!\";"} {}
                span class="tooltip-text" {"Copy to Clipboard"}
            }
        }
    }
}
