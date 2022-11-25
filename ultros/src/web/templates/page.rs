use crate::web::templates::components::footer::Footer;

use super::head::HtmlHead;
use axum::response::{Html, IntoResponse};
use lazy_static::__Deref;
use maud::{html, Markup, Render};

pub trait Page {
    fn get_name(&'_ self) -> &'_ str;
    fn get_description(&'_ self) -> Option<&'_ str> {
        None
    }
    fn get_tags(&'_ self) -> Option<&'_ str> {
        None
    }
    fn draw_body(&self) -> Markup;
}

pub struct RenderPage<T: Page>(pub(crate) T);

impl<P: Page + ?Sized> Page for Box<P> {
    fn draw_body(&self) -> Markup {
        self.deref().draw_body()
    }

    fn get_name(&'_ self) -> &'_ str {
        self.deref().get_name()
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
        let header = HtmlHead::new(self.0.get_name());
        html! {
          (header)
          body {
            (self.0.draw_body())
            ((Footer{}))
          }
        }
    }
}
