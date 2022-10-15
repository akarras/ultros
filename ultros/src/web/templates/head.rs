use maud::{html, Render, DOCTYPE};

pub(crate) struct HtmlHead<'a> {
    title: &'a str,
}

impl<'a> HtmlHead<'a> {
    pub(crate) fn new(title: &'a str) -> Self {
        Self { title }
    }
}

impl<'a> Render for HtmlHead<'a> {
    fn render(&self) -> maud::Markup {
        html! {
          (DOCTYPE)
          head {
            title { (self.title) };
            link rel="stylesheet" href="/static/main.css";
            script src="/static/search.js" {};
            link rel="manifest" href="/static/site.webmanifest" {};
          }
        }
    }
}
