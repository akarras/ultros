use chrono::{NaiveDateTime, TimeZone, Utc};
use lazy_static::lazy_static;
use maud::{PreEscaped, Render};
use timeago::{English, Formatter};

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
