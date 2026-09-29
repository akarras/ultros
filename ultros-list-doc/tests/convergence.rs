//! Three peers apply seeded random edits and exchange updates in random
//! orders. After every round they must agree on rows and meta. No `rand`
//! dependency: a xorshift keeps the crate's dependency list to Loro.

use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{ListDocument, MetaSnapshot, Quality, RowKey, RowSnapshot};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const QUALITIES: [Quality; 3] = [Quality::Any, Quality::Hq, Quality::Nq];

fn seed_doc() -> ListDocument {
    ListDocument::from_rows(
        MetaSnapshot {
            name: "seed".to_string(),
            scope: Some(AnySelector::Datacenter(5)),
        },
        &[RowSnapshot {
            key: RowKey::new(10, None),
            need: 2,
            acquired: 0,
            target: None,
        }],
    )
}

fn random_op(doc: &ListDocument, rng: &mut XorShift) {
    let keys: Vec<RowKey> = doc.rows().into_iter().map(|r| r.key).collect();
    let pick = |rng: &mut XorShift| keys[rng.below(keys.len() as u64) as usize];
    match rng.below(8) {
        0 => {
            let key = RowKey {
                item_id: 10 + rng.below(6) as i32,
                quality: QUALITIES[rng.below(3) as usize],
            };
            doc.add_row(key, 1 + rng.below(5) as i64, None).unwrap();
        }
        1 if !keys.is_empty() => {
            let _ = doc.remove_row(&pick(rng));
        }
        2 if !keys.is_empty() => {
            let _ = doc.set_need(&pick(rng), rng.below(20) as i64);
        }
        3 if !keys.is_empty() => {
            let _ = doc.add_acquired(&pick(rng), 1 + rng.below(3) as i64);
        }
        4 if !keys.is_empty() => {
            let _ = doc.set_target(&pick(rng), Some(rng.below(1000) as i64));
        }
        5 if !keys.is_empty() => {
            let _ = doc.set_quality(&pick(rng), QUALITIES[rng.below(3) as usize]);
        }
        6 => doc.rename(&format!("name-{}", rng.below(100))).unwrap(),
        _ => doc
            .set_scope(AnySelector::World(rng.below(200) as i32))
            .unwrap(),
    }
}

#[test]
fn three_peers_converge_after_random_edits_in_every_order() {
    for seed in 1..=32u64 {
        let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let snapshot = seed_doc().export_snapshot().unwrap();
        let peers: Vec<ListDocument> = (0..3)
            .map(|_| ListDocument::from_snapshot(&snapshot).unwrap())
            .collect();
        for round in 0..4 {
            for peer in &peers {
                for _ in 0..(1 + rng.below(4)) {
                    random_op(peer, &mut rng);
                }
            }
            let updates: Vec<Vec<u8>> = peers.iter().map(|p| p.export_all().unwrap()).collect();
            for peer in &peers {
                let mut order: Vec<usize> = (0..updates.len()).collect();
                for i in (1..order.len()).rev() {
                    let j = rng.below((i + 1) as u64) as usize;
                    order.swap(i, j);
                }
                for i in order {
                    peer.import(&updates[i]).unwrap();
                }
            }
            let rows = peers[0].rows();
            let meta = peers[0].meta();
            for (i, peer) in peers.iter().enumerate().skip(1) {
                assert_eq!(
                    peer.rows(),
                    rows,
                    "seed {seed} round {round}: peer {i} rows diverged"
                );
                assert_eq!(
                    peer.meta(),
                    meta,
                    "seed {seed} round {round}: peer {i} meta diverged"
                );
            }
        }
    }
}

#[test]
fn concurrent_acquired_ticks_both_count() {
    let key = RowKey::new(10, None);
    let a = seed_doc();
    let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
    a.add_acquired(&key, 2).unwrap();
    b.add_acquired(&key, 3).unwrap();
    a.import(&b.export_since(&a.version()).unwrap()).unwrap();
    b.import(&a.export_since(&b.version()).unwrap()).unwrap();
    assert_eq!(a.row(&key).unwrap().acquired, 5);
    assert_eq!(b.row(&key).unwrap().acquired, 5);
}

#[test]
fn a_concurrent_remove_wins_over_an_edit_inside_the_row() {
    let key = RowKey::new(10, None);
    let a = seed_doc();
    let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
    a.remove_row(&key).unwrap();
    b.set_need(&key, 50).unwrap();
    a.import(&b.export_since(&a.version()).unwrap()).unwrap();
    b.import(&a.export_since(&b.version()).unwrap()).unwrap();
    assert_eq!(a.rows(), b.rows());
    assert!(a.row(&key).is_none(), "spec section 1: remove wins");
}
