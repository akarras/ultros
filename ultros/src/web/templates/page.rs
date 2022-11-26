use crate::web::templates::components::footer::Footer;

use super::head::HtmlHead;
use axum::response::{Html, IntoResponse};
use lazy_static::__Deref;
use maud::{html, Markup, Render};

pub trait Page {
    fn get_name(&'_ self) -> String;
    fn get_description(&'_ self) -> Option<String> {
        None
    }
    fn get_tags(&'_ self) -> Option<String> {
        None
    }
    fn draw_body(&self) -> Markup;
}

pub struct RenderPage<T: Page>(pub(crate) T);

impl<P: Page + ?Sized> Page for Box<P> {
    fn draw_body(&self) -> Markup {
        self.deref().draw_body()
    }

    fn get_name(&'_ self) -> String {
        self.deref().get_name()
    }

    fn get_description(&'_ self) -> Option<String> {
        self.deref().get_description()
    }

    fn get_tags(&'_ self) -> Option<String> {
        self.deref().get_tags()
    }
}

impl<T> IntoResponse for RenderPage<T>
where
    T: Page,
{
    fn into_response(self) -> axum::response::Response {
        Html(self.render().0).into_response()
    }
}

impl<T> Render for RenderPage<T>
where
    T: Page,
{
    fn render(&self) -> Markup {
        let page = &self.0;
        let description = page.get_description();
        let keywords = page.get_tags();
        let header = HtmlHead {
            title: &page.get_name(),
            description: description.as_ref().map(|s| s.as_str()),
            keywords: keywords.as_ref().map(|s| s.as_str()),
        };
        html! {
          (header)
          body {
            (self.0.draw_body())
            ((Footer{}))
          }
        }
    }
}
