use maud::{html, Render, DOCTYPE};

pub(crate) struct HtmlHead<'a> {
    pub(crate) title: &'a str,
    pub(crate) description: Option<&'a str>,
    pub(crate) keywords: Option<&'a str>,
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
            link rel="stylesheet" href="/static/fa/css/all.min.css" {};
            @if let Some(description) = self.description {
                meta name="description" content=((description)) {};
            }
            @if let Some(keywords) = self.keywords {
                meta name="keywords" content=((keywords)) {};
            }
            meta charset="utf-8"{};
            meta name="viewport" content="width=device-width, initial-scale=1.0" {};
          }
        }
    }
}
