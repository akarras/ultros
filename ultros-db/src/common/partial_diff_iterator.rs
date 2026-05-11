/// Assuming two sorted sets of differing types this iterator will yield values that are different from either set
pub struct PartialDiffIterator<A, B, C, D>
where
    A: Iterator<Item = C>,
    B: Iterator<Item = D>,
    C: PartialOrd<D>,
{
    left: A,
    right: B,
    left_current: Option<C>,
    right_current: Option<D>,
}

#[derive(Eq, PartialEq, PartialOrd, Ord, Debug)]
pub enum DiffItem<C, D> {
    Same(C, D),
    Left(C),
    Right(D),
}

impl<C, D> DiffItem<C, D> {
    pub fn left(self) -> Option<C> {
        match self {
            DiffItem::Left(l) => Some(l),
            _ => None,
        }
    }

    pub fn right(self) -> Option<D> {
        match self {
            DiffItem::Right(r) => Some(r),
            _ => None,
        }
    }

    pub fn same(self) -> Option<(C, D)> {
        match self {
            DiffItem::Same(c, d) => Some((c, d)),
            _ => None,
        }
    }
}

impl<A, B, C, D> From<(A, B)> for PartialDiffIterator<A, B, C, D>
where
    A: Iterator<Item = C>,
    B: Iterator<Item = D>,
    C: PartialOrd<D>,
{
    fn from((a, b): (A, B)) -> Self {
        PartialDiffIterator::new(a, b)
    }
}

impl<A, B, C, D> PartialDiffIterator<A, B, C, D>
where
    A: Iterator<Item = C>,
    B: Iterator<Item = D>,
    C: PartialOrd<D>,
{
    pub fn new(iter1: A, iter2: B) -> Self {
        Self {
            left: iter1,
            right: iter2,
            left_current: None,
            right_current: None,
        }
    }
}

impl<A, B, C, D> Iterator for PartialDiffIterator<A, B, C, D>
where
    A: Iterator<Item = C>,
    B: Iterator<Item = D>,
    C: PartialOrd<D>,
{
    type Item = DiffItem<C, D>;
    fn next(&mut self) -> Option<Self::Item> {
        use DiffItem::*;
        let Self {
            left_current,
            right_current,
            left,
            right,
        } = self;

        // try to advance our iterators
        if left_current.is_none() {
            *left_current = left.next();
        }
        if right_current.is_none() {
            *right_current = right.next();
        }

        match (left_current.take(), right_current.take()) {
            (None, None) => None,
            (None, Some(right)) => Some(DiffItem::Right(right)),
            (Some(left), None) => Some(DiffItem::Left(left)),
            (Some(left), Some(right)) => {
                match left.partial_cmp(&right) {
                    Some(std::cmp::Ordering::Less) => {
                        // store the right value back into our store
                        self.right_current = Some(right);
                        Some(Left(left))
                    }
                    Some(std::cmp::Ordering::Equal) => Some(Same(left, right)),
                    Some(std::cmp::Ordering::Greater) => {
                        self.left_current = Some(left);
                        Some(Right(right))
                    }
                    None => {
                        // in the case of a none, assume that they are the same
                        Some(Same(left, right))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::DiffItem;
    use super::PartialDiffIterator;
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct TestA {
        name: &'static str,
        id: i32,
    }
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct TestB {
        name: &'static str,
    }

    impl PartialEq<TestB> for TestA {
        fn eq(&self, other: &TestB) -> bool {
            self.name == other.name
        }
    }

    impl PartialOrd<TestB> for TestA {
        fn partial_cmp(&self, other: &TestB) -> Option<std::cmp::Ordering> {
            Some(self.name.cmp(other.name))
        }
    }

    #[test]
    fn test_diff() {
        let a_set = [
            TestA { name: "abc", id: 1 },
            TestA { name: "dbc", id: 2 },
            TestA { name: "xyz", id: 3 },
        ];
        let b_set = [TestB { name: "dbc" }, TestB { name: "zzz" }];
        let iter = PartialDiffIterator::from((a_set.iter(), b_set.iter()));
        let results: Vec<_> = iter.collect();
        assert_eq!(
            results,
            vec![
                DiffItem::Left(&a_set[0]),
                DiffItem::Same(&a_set[1], &b_set[0]),
                DiffItem::Left(&a_set[2]),
                DiffItem::Right(&b_set[1])
            ]
        );
    }

    #[test]
    fn empty_inputs_yield_nothing() {
        let a_set: [TestA; 0] = [];
        let b_set: [TestB; 0] = [];
        let iter = PartialDiffIterator::from((a_set.iter(), b_set.iter()));
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn left_only_emits_all_as_left() {
        let a_set = [TestA { name: "a", id: 1 }, TestA { name: "b", id: 2 }];
        let b_set: [TestB; 0] = [];
        let iter = PartialDiffIterator::from((a_set.iter(), b_set.iter()));
        let results: Vec<_> = iter.collect();
        assert_eq!(
            results,
            vec![DiffItem::Left(&a_set[0]), DiffItem::Left(&a_set[1])]
        );
    }

    #[test]
    fn right_only_emits_all_as_right() {
        let a_set: [TestA; 0] = [];
        let b_set = [TestB { name: "a" }, TestB { name: "b" }];
        let iter = PartialDiffIterator::from((a_set.iter(), b_set.iter()));
        let results: Vec<_> = iter.collect();
        assert_eq!(
            results,
            vec![DiffItem::Right(&b_set[0]), DiffItem::Right(&b_set[1])]
        );
    }

    #[test]
    fn identical_sets_collapse_to_all_same() {
        let a_set = [TestA { name: "x", id: 1 }, TestA { name: "y", id: 2 }];
        let b_set = [TestB { name: "x" }, TestB { name: "y" }];
        let iter = PartialDiffIterator::from((a_set.iter(), b_set.iter()));
        let results: Vec<_> = iter.collect();
        assert_eq!(
            results,
            vec![
                DiffItem::Same(&a_set[0], &b_set[0]),
                DiffItem::Same(&a_set[1], &b_set[1]),
            ]
        );
    }

    #[test]
    fn diff_item_accessors_pick_the_matching_variant() {
        let a = TestA { name: "x", id: 1 };
        let b = TestB { name: "x" };

        // Same: only `.same()` returns Some
        let same: DiffItem<&TestA, &TestB> = DiffItem::Same(&a, &b);
        assert!(same.left().is_none());
        let same: DiffItem<&TestA, &TestB> = DiffItem::Same(&a, &b);
        assert!(same.right().is_none());
        let same: DiffItem<&TestA, &TestB> = DiffItem::Same(&a, &b);
        assert_eq!(same.same(), Some((&a, &b)));

        // Left: only `.left()` returns Some
        let left: DiffItem<&TestA, &TestB> = DiffItem::Left(&a);
        assert_eq!(left.left(), Some(&a));
        let left: DiffItem<&TestA, &TestB> = DiffItem::Left(&a);
        assert!(left.right().is_none());

        // Right: only `.right()` returns Some
        let right: DiffItem<&TestA, &TestB> = DiffItem::Right(&b);
        assert_eq!(right.right(), Some(&b));
        let right: DiffItem<&TestA, &TestB> = DiffItem::Right(&b);
        assert!(right.left().is_none());
    }
}
