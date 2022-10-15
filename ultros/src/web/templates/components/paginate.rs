use std::collections::HashMap;

use maud::{html, Render};

pub(crate) struct Paginate<'a, T> {
    values: &'a [T],
    /// number of items to show in a page
    page_size: usize,
    /// Current page
    current_page: usize,
    /// The pagination control will render variables as ?page={page}, this string provides an override for
    /// adding queries such that other parts of the query can be preserved ?other_query=value&page-1
    query_str: String,
}

impl<'a, T> Paginate<'a, T> {
    /// Creates a new pagination control
    pub(crate) fn new(
        values: &'a [T],
        page_size: usize,
        current_page: usize,
        query_str: String,
    ) -> Self {
        Self {
            values,
            page_size,
            current_page,
            query_str,
        }
    }

    pub(crate) fn get_page(&self) -> &[T] {
        let page = self.current_page.saturating_sub(1);
        let start_index = page * self.page_size;
        let end_index = start_index + self.page_size;
        &self.values[start_index..end_index.min(self.values.len())]
    }
}

/// The render trait for the paginate control should just draw a page selection control
impl<'a, T> Render for Paginate<'a, T> {
    fn render(&self) -> maud::Markup {
        let num_pages = self.values.len() / self.page_size;
        let query_prefix = if self.query_str.is_empty() {
            "?page=".to_string()
        } else {
            format!("?{}&page=", self.query_str)
        };
        html! {
           div class="flex-row" {
            @for page in 1..=num_pages {
                @if page == 1 || (page >= self.current_page.saturating_sub(5) && page <= self.current_page.saturating_add(5)) || page == num_pages {
                    a href={((query_prefix)) ((page))} class={ @if self.current_page == page { "btn-secondary" } @else { "btn" } } {
                        ((page))
                    }
                }
            }
           }
        }
    }
}
