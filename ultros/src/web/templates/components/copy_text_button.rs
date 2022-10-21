use maud::{html, Render};

pub(crate) struct CopyTextButton<'a> {
    pub(crate) text: &'a str,
}

impl<'a> Render for CopyTextButton<'a> {
    fn render(&self) -> maud::Markup {
        html! {
            i class="fa-regular fa-clipboard" title="copy text" onclick={"navigator.clipboard.writeText(\"" ((self.text)) "\"); this.title = \"copied\";"} {}
        }
    }
}
