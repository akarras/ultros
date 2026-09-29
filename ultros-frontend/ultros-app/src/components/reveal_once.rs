//! One scroll-to-row per deep link.
//!
//! `?item=<id>` asks a grid page to reveal that item's row. The row's index
//! moves every time live prices re-sort the table, and re-revealing on each
//! move would yank the scroll position out from under the reader — so the
//! reveal fires once per `item` value, the first time the row is present,
//! and re-arms only when the parameter changes.

#[derive(Default, Debug)]
pub struct RevealOnce {
    item: Option<i32>,
    done: bool,
}

impl RevealOnce {
    /// `Some(index)` at most once per distinct `item` value, the first time
    /// `position` resolves it. A new `item` re-arms; `None` disarms.
    pub fn next(
        &mut self,
        item: Option<i32>,
        position: impl FnOnce(i32) -> Option<usize>,
    ) -> Option<usize> {
        if item != self.item {
            self.item = item;
            self.done = false;
        }
        let item = item?;
        if self.done {
            return None;
        }
        let index = position(item)?;
        self.done = true;
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveals_once_when_the_row_appears() {
        let mut once = RevealOnce::default();
        assert_eq!(once.next(Some(7), |_| None), None, "row not there yet");
        assert_eq!(once.next(Some(7), |_| Some(3)), Some(3));
        assert_eq!(
            once.next(Some(7), |_| Some(9)),
            None,
            "re-sort must not re-reveal"
        );
    }

    #[test]
    fn a_new_item_re_arms() {
        let mut once = RevealOnce::default();
        assert_eq!(once.next(Some(7), |_| Some(3)), Some(3));
        assert_eq!(once.next(Some(8), |_| Some(1)), Some(1));
        assert_eq!(
            once.next(None, |_| Some(0)),
            None,
            "no param, nothing to reveal"
        );
        assert_eq!(
            once.next(Some(8), |_| Some(1)),
            Some(1),
            "param came back: reveal again"
        );
    }
}
