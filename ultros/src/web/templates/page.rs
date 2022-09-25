use super::head::HtmlHead;
use axum::response::{Html, IntoResponse};
use maud::{html, Markup, Render};

pub(crate) trait Page {
    fn get_name<'a>(&self) -> &'a str;
    fn draw_body(&self) -> Markup;
}

pub(crate) struct RenderPage<T: Page>(pub(crate) T);

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
          }
        }
    }
}
