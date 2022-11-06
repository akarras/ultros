use chrono::{Utc, NaiveDateTime, TimeZone};
use maud::{Render, PreEscaped};
use lazy_static::lazy_static;
use timeago::{Formatter, English};

pub(crate) struct TimeSince(pub(crate) NaiveDateTime);

impl Render for TimeSince {
    fn render(&self) -> maud::Markup {
        let now = Utc::now();
        lazy_static! {
          static ref FORMATTER: Formatter<English> = Formatter::new();
        };
        let start = Utc.from_utc_datetime(&self.0);
        PreEscaped(FORMATTER.convert_chrono(start, now))
    }
}

