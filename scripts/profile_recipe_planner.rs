//! Standalone, deterministic route-search probe. See docs/recipe-planner-performance.md.
//! Includes the real engine without compiling the application or loading game data.
use std::{cell::RefCell, collections::BTreeMap, hint::black_box};

#[allow(dead_code)]
#[path = "../ultros-frontend/ultros-calc/src/recipe_planner.rs"]
mod planner;

struct Fixture {
    materials: Vec<planner::Material>,
    market: BTreeMap<i32, Vec<planner::Offer>>,
    context: planner::RouteContext,
    locked_context: planner::RouteContext,
}

thread_local! {
    static FIXTURE: RefCell<Option<Fixture>> = const { RefCell::new(None) };
}

/// Setup is outside the measured interval. Markets are synthetic, not a replay
/// of the reporter's recipe. Every world supplies every material; prices vary
/// by item and world to exercise routes rather than always choosing home.
#[unsafe(no_mangle)]
pub extern "C" fn setup(worlds: i32, items: i32, needed: i32, listings: i32) {
    assert!((1..=64).contains(&worlds));
    assert!((1..=128).contains(&items));
    assert!((1..=10_000).contains(&needed));
    assert!((1..=100).contains(&listings));
    let materials: Vec<_> = (1..=items)
        .map(|item| planner::Material {
            item,
            needed: i64::from(needed),
            ..Default::default()
        })
        .collect();
    let market: BTreeMap<_, _> = (1..=items)
        .map(|item| {
            let offers = (1..=worlds)
                .flat_map(|world| {
                    (0..listings).map(move |n| planner::Offer {
                        id: item * 100_000 + world * 1000 + n,
                        world,
                        quantity: i64::from(1 + (n * 17 + item * 3 + world * 7) % 99),
                        price: i64::from(100 + (world * 137 + item * 251 + n * 31) % 5000),
                    })
                })
                .collect();
            (item, offers)
        })
        .collect();
    let context = planner::RouteContext {
        home: 1,
        datacenters: (1..=worlds).map(|w| (w, (w - 1) / 8)).collect(),
        ..Default::default()
    };
    let initial = planner::compare_routes(&materials, &market, &BTreeMap::new(), &context);
    let mut locked_context = context.clone();
    if let Some(best) = initial.best_value() {
        if let Some((item, offer)) = initial.cards[best]
            .purchases
            .iter()
            .find_map(|(item, p)| p.offers.first().map(|o| (*item, o.clone())))
        {
            locked_context.locked.insert(item, vec![offer]);
        }
    }
    FIXTURE.with(|f| {
        *f.borrow_mut() = Some(Fixture {
            materials,
            market,
            context,
            locked_context,
        });
    });
}

/// Each invocation starts a fresh route search, just like comparison's memo
/// after a checkbox change. Returns a checksum to keep all results observable.
#[unsafe(no_mangle)]
pub extern "C" fn search(locked: bool) -> i64 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        let f = f.as_ref().expect("call setup first");
        let result = planner::compare_routes(
            black_box(&f.materials),
            black_box(&f.market),
            &BTreeMap::new(),
            if locked {
                &f.locked_context
            } else {
                &f.context
            },
        );
        black_box(result.cards.iter().map(|p| p.cost).sum())
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("worlds,items,needed,listings_per_world,locked,median_ms,checksum");
    for (worlds, items, needed, listings) in [
        (8, 6, 10, 20),
        (8, 7, 9999, 90),
        (8, 24, 100, 20),
        (32, 24, 100, 20),
        (32, 24, 1000, 20),
        (32, 64, 100, 20),
    ] {
        setup(worlds, items, needed, listings);
        for locked in [false, true] {
            search(locked);
            let mut times = Vec::new();
            let mut checksum = 0;
            for _ in 0..5 {
                let start = std::time::Instant::now();
                checksum = search(locked);
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            println!(
                "{worlds},{items},{needed},{listings},{locked},{:.2},{checksum}",
                times[2]
            );
        }
    }
}
