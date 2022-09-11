/// Assuming two sorted sets of differing types this iterator will yield disjointed that have added or removed from either set
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
pub enum Diff<C, D> {
    Same(C, D),
    Left(C),
    Right(D),
}

impl<A, B, C, D> From<(A, B)> for PartialDiffIterator<A, B, C, D>
where
    A: Iterator<Item = C>,
    B: Iterator<Item = D>,
    C: PartialOrd<D>,
{
    fn from((a, b): (A, B)) -> Self {
        PartialDiffIterator::diff_iter(a, b)
    }
}

impl<A, B, C, D> PartialDiffIterator<A, B, C, D>
where
    A: Iterator<Item = C>,
    B: Iterator<Item = D>,
    C: PartialOrd<D>,
{
    pub fn diff_iter(iter1: A, iter2: B) -> Self {
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
    type Item = Diff<C, D>;
    fn next(&mut self) -> Option<Self::Item> {
        use Diff::*;
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
            (None, Some(right)) => Some(Diff::Right(right)),
            (Some(left), None) => Some(Diff::Left(left)),
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
    use crate::partial_diff_iterator::Diff;

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
            Some(self.name.cmp(&other.name))
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
                Diff::Left(&a_set[0]),
                Diff::Same(&a_set[1], &b_set[0]),
                Diff::Left(&a_set[2]),
                Diff::Right(&b_set[1])
            ]
        );
    }
}
