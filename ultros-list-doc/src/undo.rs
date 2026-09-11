//! Local undo over one document (spec section 3.3). Remote imports are not
//! local operations, so they never enter the stack.

use loro::UndoManager;

use crate::document::{DocError, ListDocument};

pub struct ListUndo {
    inner: UndoManager,
}

impl ListUndo {
    /// Edits closer together than this merge into one undo step. Zero: every
    /// committed action is its own step, however quickly it followed the
    /// previous one. Loro's time-based merge applies across explicit groups
    /// too, so any positive interval would fold two quick cell commits (or
    /// two Enter presses in the catalog search) into one Ctrl+Z, which is
    /// exactly the unpredictability issue #1430 removes. Multi-row edits
    /// still become one step through [`Self::group`].
    pub const MERGE_INTERVAL_MS: i64 = 0;
    pub const MAX_STEPS: usize = 100;

    pub fn new(doc: &ListDocument) -> Self {
        Self::with_merge_interval(doc, Self::MERGE_INTERVAL_MS)
    }

    /// The default interval is already `0`; this stays for callers that
    /// want to opt back into time-based merging.
    pub fn with_merge_interval(doc: &ListDocument, interval_ms: i64) -> Self {
        let mut inner = UndoManager::new(doc.inner());
        inner.set_merge_interval(interval_ms);
        inner.set_max_undo_steps(Self::MAX_STEPS);
        Self { inner }
    }

    /// `Ok(false)` when there was nothing to undo.
    pub fn undo(&mut self) -> Result<bool, DocError> {
        Ok(self.inner.undo()?)
    }

    pub fn redo(&mut self) -> Result<bool, DocError> {
        Ok(self.inner.redo()?)
    }

    pub fn can_undo(&self) -> bool {
        self.inner.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.inner.can_redo()
    }

    /// Run several document edits as one undo step (bulk HQ, imports).
    ///
    /// The group is closed even if `f` panics: an open group would swallow
    /// every later edit into one step for the rest of the session.
    pub fn group<R>(&mut self, f: impl FnOnce() -> Result<R, DocError>) -> Result<R, DocError> {
        struct CloseGroup<'a>(&'a mut UndoManager);

        impl Drop for CloseGroup<'_> {
            fn drop(&mut self) {
                self.0.group_end();
            }
        }

        self.inner.group_start()?;
        let _guard = CloseGroup(&mut self.inner);
        f()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Quality, RowKey};
    use crate::snapshot::{MetaSnapshot, RowSnapshot};
    use ultros_api_types::world_helper::AnySelector;

    /// What Loro does when this device undoes a write that another peer
    /// overwrote in the meantime. Pinned by `undo_after_remote_overwrite`
    /// on first run; the spec adopts whichever value Loro produces, and this
    /// constant documents it. If a Loro bump changes it, update both.
    /// Loro restores the local prior value even over a later remote write.
    const REMOTE_OVERWRITE_UNDO_RESULT: i64 = 1;

    fn doc() -> (ListDocument, RowKey) {
        let key = RowKey::new(10, None);
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "t".into(),
                scope: Some(AnySelector::World(1)),
            },
            &[RowSnapshot {
                key,
                need: 1,
                acquired: 0,
                target: None,
            }],
        );
        (doc, key)
    }

    #[test]
    fn undo_and_redo_walk_need_edits() {
        let (doc, key) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        assert!(!undo.can_undo());
        doc.set_need(&key, 5).unwrap();
        doc.set_need(&key, 9).unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 5);
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 1);
        assert!(!undo.undo().unwrap());
        assert!(undo.redo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 5);
    }

    /// The production constructor: two commits in the same millisecond are
    /// still two steps, and a group is still one.
    #[test]
    fn default_manager_keeps_rapid_commits_as_separate_steps() {
        let (doc, key) = doc();
        let mut undo = ListUndo::new(&doc);
        undo.group(|| doc.set_need(&key, 5)).unwrap();
        undo.group(|| doc.set_need(&key, 9)).unwrap();
        let other = RowKey::new(11, None);
        undo.group(|| {
            doc.add_row(other, 2, None)?;
            doc.set_target(&other, Some(30))
        })
        .unwrap();
        assert!(undo.undo().unwrap());
        assert!(doc.row(&other).is_none(), "the grouped add is one step");
        assert_eq!(doc.row(&key).unwrap().need, 9);
        assert!(undo.undo().unwrap());
        assert_eq!(
            doc.row(&key).unwrap().need,
            5,
            "rapid commits undo one at a time"
        );
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 1);
        assert!(!undo.can_undo());
        assert!(undo.can_redo());
    }

    #[test]
    fn undo_of_a_removal_restores_every_field() {
        let (doc, key) = doc();
        doc.set_target(&key, Some(40)).unwrap();
        doc.add_acquired(&key, 1).unwrap();
        let before = doc.row(&key).unwrap();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        doc.remove_row(&key).unwrap();
        assert!(doc.row(&key).is_none());
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key), Some(before));
    }

    #[test]
    fn undo_of_an_add_removes_the_row() {
        let (doc, _) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let new_key = RowKey::new(11, Some(true));
        doc.add_row(new_key, 2, None).unwrap();
        assert!(undo.undo().unwrap());
        assert!(doc.row(&new_key).is_none());
        assert!(undo.redo().unwrap());
        assert_eq!(doc.row(&new_key).unwrap().need, 2);
    }

    #[test]
    fn undo_of_a_counter_increment_subtracts_it() {
        let (doc, key) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        doc.add_acquired(&key, 3).unwrap();
        doc.add_acquired(&key, 2).unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 3);
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
    }

    #[test]
    fn undo_of_a_quality_move_puts_the_row_back() {
        let (doc, key) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let moved = doc.set_quality(&key, Quality::Hq).unwrap();
        assert!(undo.undo().unwrap());
        assert!(doc.row(&moved).is_none());
        assert_eq!(doc.row(&key).unwrap().need, 1);
    }

    #[test]
    fn a_group_undoes_as_one_step() {
        let (doc, key) = doc();
        let other = RowKey::new(11, None);
        doc.add_row(other, 1, None).unwrap();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        undo.group(|| {
            doc.set_quality(&key, Quality::Hq)?;
            doc.set_quality(&other, Quality::Hq)?;
            Ok(())
        })
        .unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().key.quality, Quality::Any);
        assert_eq!(doc.row(&other).unwrap().key.quality, Quality::Any);
        assert!(!undo.can_undo());
    }

    #[test]
    fn remote_imports_never_enter_the_stack() {
        let (a, key) = doc();
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let mut undo = ListUndo::with_merge_interval(&a, 0);
        b.set_need(&key, 8).unwrap();
        a.import(&b.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(a.row(&key).unwrap().need, 8);
        assert!(!undo.can_undo());
        assert!(!undo.undo().unwrap());
        assert_eq!(a.row(&key).unwrap().need, 8);
    }

    /// Pins how Loro resolves "undo my write that someone else overwrote".
    /// The one invariant that must hold either way: the undone value never
    /// comes back.
    #[test]
    fn undo_after_remote_overwrite() {
        let (a, key) = doc();
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let mut undo = ListUndo::with_merge_interval(&a, 0);
        a.set_need(&key, 5).unwrap();
        b.import(&a.export_since(&b.version()).unwrap()).unwrap();
        b.set_need(&key, 7).unwrap();
        a.import(&b.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(a.row(&key).unwrap().need, 7, "the remote write landed last");
        undo.undo().unwrap();
        let after = a.row(&key).unwrap().need;
        assert_ne!(after, 5, "undo must not resurrect the undone value");
        assert_eq!(
            after, REMOTE_OVERWRITE_UNDO_RESULT,
            "Loro's behaviour changed; update the constant and the spec's section 3.3 note"
        );
    }
}
