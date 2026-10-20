//! Local undo over one document (spec section 3.3). Remote imports are not
//! local operations, so they never enter the stack.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
};

use loro::{ContainerID, LoroValue, UndoItemMeta, UndoManager};

use crate::document::{DocError, ListDocument};
use crate::key::RowKey;

#[derive(Clone)]
struct Purchase {
    id: i64,
    key: RowKey,
    row: ContainerID,
    quantity: i64,
    active: bool,
}

// Undo/redo creates the inverse operation and transfers its tag to the other
// stack. Positive tags record purchases; negative tags reverse them.
struct PurchaseTag(Arc<AtomicI64>);

impl Drop for PurchaseTag {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

pub struct ListUndo {
    inner: UndoManager,
    doc: ListDocument,
    purchases: VecDeque<Purchase>,
    next_purchase: i64,
    tag: Arc<AtomicI64>,
    popped: Arc<Mutex<Vec<i64>>>,
    largest_span: Arc<AtomicU64>,
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
        let tag = Arc::new(AtomicI64::new(0));
        let current = tag.clone();
        let largest_span = Arc::new(AtomicU64::new(0));
        let pushed_operations = largest_span.clone();
        inner.set_on_push(Some(Box::new(move |_, span, _| {
            let count = u64::from(span.end.abs_diff(span.start));
            if interval_ms > 0 {
                // A time-merged source entry can span multiple notifications.
                // The default uses explicit groups instead; custom timed merge
                // conservatively reserves everything recorded by this manager.
                pushed_operations.fetch_add(count, Ordering::Relaxed);
            } else {
                pushed_operations.fetch_max(count, Ordering::Relaxed);
            }
            let mut meta = UndoItemMeta::new();
            meta.set_value(LoroValue::I64(current.load(Ordering::Relaxed)));
            meta
        })));
        // Loro may skip an undo step whose row was removed remotely. Use
        // each actual pop, not just the tag that was on top before undo.
        let popped = Arc::new(Mutex::new(Vec::new()));
        let changes = popped.clone();
        let current = tag.clone();
        inner.set_on_pop(Some(Box::new(move |_, _, meta| {
            let inverse = match meta.value {
                LoroValue::I64(tag) => tag.checked_neg().unwrap_or(0),
                _ => 0,
            };
            current.store(inverse, Ordering::Relaxed);
            if inverse != 0 {
                // The callback cannot return an error. Keep a poisoned queue
                // poisoned; history() reports it before any subsequent edit.
                if let Ok(mut changes) = changes.lock() {
                    changes.push(inverse);
                }
            }
        })));
        Self {
            inner,
            doc: doc.clone(),
            purchases: VecDeque::new(),
            next_purchase: 1,
            tag,
            popped,
            largest_span,
        }
    }

    /// `Ok(false)` when there was nothing to undo.
    pub fn undo(&mut self) -> Result<bool, DocError> {
        self.history(false)
    }

    pub fn redo(&mut self) -> Result<bool, DocError> {
        self.history(true)
    }

    fn tagged(&self, tag: i64) -> PurchaseTag {
        self.tag.store(tag, Ordering::Relaxed);
        PurchaseTag(self.tag.clone())
    }

    fn history(&mut self, redo: bool) -> Result<bool, DocError> {
        if self.popped.is_poisoned() {
            return Err(DocError::InvalidStructure(
                "purchase history unavailable; reopen the saved list".into(),
            ));
        }
        if if redo {
            !self.can_redo()
        } else {
            !self.can_undo()
        } {
            return Ok(false);
        }
        self.doc
            .reserve_undo(self.largest_span.load(Ordering::Relaxed))?;
        let _tag = self.tagged(0);
        let changed = if redo {
            self.inner.redo()
        } else {
            self.inner.undo()
        };
        let mut popped = self.popped.lock().map_err(|_| {
            DocError::InvalidStructure("purchase history unavailable; reopen the saved list".into())
        })?;
        for tag in popped.drain(..) {
            if let Some(purchase) = self
                .purchases
                .iter_mut()
                .find(|p| Some(p.id) == tag.checked_abs())
            {
                purchase.active = tag > 0;
            }
        }
        Ok(changed?)
    }

    /// A Shop purchase targets the exact cart row, even when other qualities
    /// of the same item exist. Auto-mark's fallback-to-another-row is separate.
    pub fn record_purchase(&mut self, key: RowKey, quantity: i64) -> Result<(), DocError> {
        let Some(before) = self.doc.row(&key) else {
            return Ok(());
        };
        if quantity <= 0 || before.acquired >= before.need {
            return Ok(());
        }
        let row = self
            .doc
            .row_identity(&key)
            .ok_or(DocError::MissingRow(key))?;
        let id = self.next_purchase;
        self.next_purchase = id.checked_add(1).ok_or(DocError::QuantityOverflow)?;
        let _tag = self.tagged(id);
        self.group(|doc| doc.add_acquired(&key, quantity))?;
        self.purchases.push_back(Purchase {
            id,
            key,
            row,
            quantity,
            active: true,
        });
        if self.purchases.len() > Self::MAX_STEPS {
            self.purchases.pop_front();
        }
        Ok(())
    }

    fn reversible(&self, purchase: &Purchase) -> bool {
        purchase.active
            && self.doc.row_identity(&purchase.key).as_ref() == Some(&purchase.row)
            && self
                .doc
                .row(&purchase.key)
                .is_some_and(|row| row.acquired >= purchase.quantity)
    }

    /// Purchase history has the same lifetime as ordinary local undo. A removed,
    /// replaced, or quality-moved row is never resurrected or redirected to a
    /// different row. Insufficient owned quantity cannot be made negative.
    pub fn can_undo_purchase(&self) -> bool {
        self.purchases.iter().rev().any(|p| self.reversible(p))
    }

    /// Record an explicit reverse delta as a new undo step, preserving all
    /// intervening edits. Unlike auto-mark, a full row is eligible for reversal.
    pub fn undo_purchase(&mut self) -> Result<bool, DocError> {
        let Some(purchase) = self
            .purchases
            .iter()
            .rev()
            .find(|p| self.reversible(p))
            .cloned()
        else {
            return Ok(false);
        };
        let tag = purchase
            .id
            .checked_neg()
            .ok_or(DocError::QuantityOverflow)?;
        let delta = purchase
            .quantity
            .checked_neg()
            .ok_or(DocError::QuantityOverflow)?;
        let _tag = self.tagged(tag);
        self.group(|doc| doc.add_acquired(&purchase.key, delta))?;
        if let Some(entry) = self.purchases.iter_mut().find(|p| p.id == purchase.id) {
            entry.active = false;
        }
        Ok(true)
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
    /// The deterministic callback runs first on an isolated candidate, then on
    /// the live document. Use the supplied document and avoid other side effects.
    pub fn group<R>(
        &mut self,
        f: impl Fn(&ListDocument) -> Result<R, DocError>,
    ) -> Result<R, DocError> {
        self.doc.preflight(|candidate| f(candidate))?;
        let document = self.doc.prechecked();
        struct CloseGroup<'a>(&'a mut UndoManager);

        impl Drop for CloseGroup<'_> {
            fn drop(&mut self) {
                self.0.group_end();
            }
        }

        let peer = document.peer_id();
        let before = document.inner().oplog_vv().get(&peer).copied().unwrap_or(0);
        self.inner.group_start()?;
        let guard = CloseGroup(&mut self.inner);
        let result = f(&document);
        drop(guard);
        let after = document.inner().oplog_vv().get(&peer).copied().unwrap_or(0);
        self.largest_span
            .fetch_max(u64::from(after.abs_diff(before)), Ordering::Relaxed);
        result
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
    fn poisoned_purchase_history_returns_an_error_without_consuming_undo() {
        let (doc, key) = doc();
        let mut undo = ListUndo::new(&doc);
        doc.set_need(&key, 5).unwrap();
        let version = doc.version();
        let undo_count = undo.inner.undo_count();
        let queue = undo.popped.clone();
        assert!(
            std::panic::catch_unwind(move || {
                let _guard = queue.lock().unwrap();
                panic!("simulate a failed purchase-history callback");
            })
            .is_err()
        );
        assert!(undo.undo().is_err());
        assert!(undo.redo().is_err());
        assert_eq!(doc.version(), version);
        assert_eq!(undo.inner.undo_count(), undo_count);
        assert!(undo.popped.is_poisoned(), "do not reuse poisoned state");
    }

    #[test]
    fn skipped_purchase_steps_do_not_tag_the_next_document_edit() {
        let (doc, key) = doc();
        let mut undo = ListUndo::new(&doc);
        doc.rename("later name").unwrap();
        undo.record_purchase(key, 1).unwrap();
        let remote = ListDocument::from_snapshot(&doc.export_snapshot().unwrap()).unwrap();
        remote.remove_row(&key).unwrap();
        doc.import(&remote.export_since(&doc.version()).unwrap())
            .unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.meta().name, "t");
        assert_eq!(undo.inner.top_redo_value(), Some(LoroValue::I64(0)));
        assert!(!undo.purchases[0].active);
        assert!(undo.redo().unwrap());
        assert_eq!(doc.meta().name, "later name");
        assert!(!undo.purchases[0].active);
    }

    #[test]
    fn purchase_undo_preserves_later_edits_and_reverses_a_full_row() {
        let (doc, key) = doc();
        let mut undo = ListUndo::new(&doc);
        undo.record_purchase(key, 1).unwrap();
        assert!(undo.can_undo_purchase());
        doc.set_target(&key, Some(500)).unwrap();
        assert!(undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
        assert_eq!(doc.row(&key).unwrap().target, Some(500));
        assert!(!undo.undo_purchase().unwrap());
    }

    #[test]
    fn purchase_history_tracks_keyboard_undo_and_redo() {
        let (doc, key) = doc();
        let mut undo = ListUndo::new(&doc);
        undo.record_purchase(key, 1).unwrap();
        doc.set_need(&key, 5).unwrap();
        assert!(undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 5);
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
        assert!(!undo.can_undo_purchase());
        // Ctrl+Z reverses the purchase reversal as one step.
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 1);
        assert!(undo.can_undo_purchase());
        assert!(undo.redo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
        assert!(!undo.can_undo_purchase());
        assert!(undo.undo().unwrap());
        assert!(undo.undo().unwrap()); // quantity edit
        assert_eq!(doc.row(&key).unwrap().need, 1);
        assert!(undo.undo().unwrap()); // original purchase
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
        assert!(!undo.can_undo_purchase());
        assert!(!undo.undo_purchase().unwrap());
        assert!(undo.redo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 1);
        assert!(undo.can_undo_purchase());
        assert!(undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
    }

    #[test]
    fn purchases_are_separate_steps_and_reverse_most_recent_first() {
        let (doc, key) = doc();
        doc.set_need(&key, 10).unwrap();
        let mut undo = ListUndo::new(&doc);
        undo.record_purchase(key, 2).unwrap();
        undo.record_purchase(key, 3).unwrap();
        doc.rename("later name").unwrap();
        assert!(undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 2);
        assert_eq!(doc.meta().name, "later name");
        assert!(undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
        assert!(!undo.can_undo_purchase());
    }

    #[test]
    fn purchase_undo_never_uses_a_different_quality_or_replacement_row() {
        let (doc, key) = doc();
        let hq = RowKey::new(key.item_id, Some(true));
        doc.add_row(hq, 3, None).unwrap();
        doc.set_acquired(&hq, 2).unwrap();
        let mut undo = ListUndo::new(&doc);
        undo.record_purchase(key, 1).unwrap();
        doc.remove_row(&key).unwrap();
        doc.add_row(key, 4, None).unwrap();
        doc.set_acquired(&key, 3).unwrap();
        assert!(!undo.can_undo_purchase());
        assert!(!undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 3);
        assert_eq!(doc.row(&hq).unwrap().acquired, 2);
    }

    #[test]
    fn purchase_reversal_preserves_remote_acquisitions() {
        let (doc, key) = doc();
        doc.set_need(&key, 10).unwrap();
        let remote = ListDocument::from_snapshot(&doc.export_snapshot().unwrap()).unwrap();
        let mut undo = ListUndo::new(&doc);
        undo.record_purchase(key, 2).unwrap();
        remote.add_acquired(&key, 3).unwrap();
        doc.import(&remote.export_since(&doc.version()).unwrap())
            .unwrap();
        assert!(undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 3);
    }

    #[test]
    fn empty_and_insufficient_purchase_history_are_no_ops() {
        let (doc, key) = doc();
        let mut undo = ListUndo::new(&doc);
        doc.rename("keep me").unwrap();
        assert!(!undo.undo_purchase().unwrap());
        assert_eq!(doc.meta().name, "keep me");
        undo.record_purchase(key, 0).unwrap();
        assert!(!undo.can_undo_purchase());
        undo.record_purchase(key, 2).unwrap();
        doc.set_acquired(&key, 1).unwrap();
        assert!(!undo.undo_purchase().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 1);
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
        undo.group(|doc| doc.set_need(&key, 5)).unwrap();
        undo.group(|doc| doc.set_need(&key, 9)).unwrap();
        let other = RowKey::new(11, None);
        undo.group(|doc| {
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
        let before_inverse: i64 = doc.inner().oplog_vv().values().map(|n| i64::from(*n)).sum();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key), Some(before));
        let after_inverse: i64 = doc.inner().oplog_vv().values().map(|n| i64::from(*n)).sum();
        assert!(
            after_inverse - before_inverse <= 8,
            "the pinned decoder's full-row inverse exceeded the admission reserve"
        );
        assert_eq!(
            ListDocument::from_snapshot(&doc.export_snapshot().unwrap())
                .unwrap()
                .rows(),
            doc.rows()
        );
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
        undo.group(|doc| {
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

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn conservative_byte_refusal_preserves_undo_and_observers() {
        let raw = loro::LoroDoc::new();
        raw.get_map("meta").insert("schema", 1_i64).unwrap();
        raw.get_map("meta")
            .insert("name", "x".repeat(2 * 1024 * 1024))
            .unwrap();
        raw.commit();
        let document = ListDocument::from_snapshot(
            &raw.export(loro::ExportMode::shallow_snapshot(&raw.state_frontiers()))
                .unwrap(),
        )
        .unwrap();
        let mut undo = ListUndo::new(&document);
        document.rename("short").unwrap();
        let before = document.version();
        let count = undo.inner.undo_count();
        let events = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = events.clone();
        let _subscription = document.on_change(move || {
            observed.fetch_add(1, Ordering::Relaxed);
        });
        assert!(undo.undo().is_err());
        assert_eq!(undo.inner.undo_count(), count);
        assert_eq!(document.version(), before);
        assert_eq!(events.load(Ordering::Relaxed), 0);
        ListDocument::from_snapshot(&document.export_snapshot().unwrap()).unwrap();
    }

    #[test]
    fn a_small_inverse_does_not_reserve_the_entire_lifetime_payload() {
        let document = ListDocument::new();
        let key = RowKey::new(1, None);
        document.add_row(key, 1, None).unwrap();
        let meta = document.inner().get_map("meta");
        for index in 0..10_000 {
            meta.insert("name", if index % 2 == 0 { "a" } else { "b" })
                .unwrap();
            document.commit();
        }
        let mut undo = ListUndo::new(&document);
        document.set_need(&key, 7).unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(document.row(&key).unwrap().need, 1);
        ListDocument::from_snapshot(&document.export_snapshot().unwrap()).unwrap();
    }

    #[test]
    fn refused_group_and_undo_do_not_mutate_or_consume_the_stack() {
        let document = ListDocument::new();
        let meta = document.inner().get_map("meta");
        for index in 1..99_999 {
            meta.insert("name", if index % 2 == 0 { "a" } else { "b" })
                .unwrap();
            document.commit();
        }
        let mut undo = ListUndo::new(&document);
        let before = document.version();
        assert!(
            undo.group(|candidate| {
                candidate.rename("first")?;
                candidate.rename("second")
            })
            .is_err()
        );
        assert_eq!(document.version(), before);
        assert!(!undo.can_undo());
        document.rename("last accepted edit").unwrap();
        let before = document.version();
        let count = undo.inner.undo_count();
        assert!(undo.undo().is_err());
        assert_eq!(undo.inner.undo_count(), count);
        assert!(undo.can_undo());
        assert_eq!(document.version(), before);
        assert_eq!(document.meta().name, "last accepted edit");
        ListDocument::from_snapshot(&document.export_snapshot().unwrap()).unwrap();
    }
}
