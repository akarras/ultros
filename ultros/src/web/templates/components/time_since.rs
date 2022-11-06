use chrono::{Utc, DateTime};
use maud::{Render, PreEscaped};
use lazy_static::lazy_static;
use timeago::{Formatter, English};

struct TimeSince(DateTime<Utc>);

impl Render for TimeSince {
    fn render(&self) -> maud::Markup {
        let now = Utc::now();
        lazy_static! {
          static ref FORMATTER: Formatter<English> = Formatter::new();
        };
        PreEscaped(FORMATTER.convert_chrono(self.0, now))
    }
}

