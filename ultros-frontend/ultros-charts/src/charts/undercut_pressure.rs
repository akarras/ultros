//! Undercut-pressure pane: a short lane under the price chart that shares
//! its time axis. A state ribbon, stacked cut/trim bars, the item's typical
//! level (dashed) and a sales line. Everything is a count per bucket, so one
//! y axis. Horizontal geometry mirrors `price_history` exactly.
use crate::scale::{LinearScale, TimeScale};
use crate::scene::{Color, Node, Scene, Stroke};
use crate::svg::rects_path_d;
use crate::theme::Theme;
use ultros_api_types::undercut_pressure::{PressureBucket, PressureState, UndercutPressure};

pub const PANE_MARGIN_LEFT: f32 = 68.0;
pub const PANE_MARGIN_RIGHT: f32 = 16.0;
const RIBBON_TOP: f32 = 2.0;
const RIBBON_HEIGHT: f32 = 6.0;
const RIBBON_GAP: f32 = 6.0;
const BOTTOM_PAD: f32 = 4.0;

pub fn pane_height(width: f32) -> f32 {
    (width * 0.16).clamp(90.0, 150.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressurePalette {
    pub cut: Color,
    pub trim: Color,
    pub sales: Color,
    pub baseline: Color,
    pub war: Color,
    pub churn: Color,
    pub calm: Color,
    pub unknown: Color,
    pub grid: Color,
}

impl Default for PressurePalette {
    fn default() -> Self {
        Self {
            cut: Color::hex("#e34948"),
            trim: Color::hex("#eda100"),
            sales: Color::hex("#eb6834"),
            baseline: Color::hex("#898781"),
            // One step darker than `cut` so the ribbon and bars batch into
            // separate paths and `color_attr` can theme them independently.
            war: Color::hex("#d23c3b"),
            churn: Color::hex("#6b6875"),
            calm: Color::hex("#1baf7a"),
            unknown: Color::hex("#3a3644"),
            grid: Color::hex("#2c2c2a"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressurePaneOptions {
    pub width: f32,
    pub height: f32,
    /// The price chart's `time_domain`, unix seconds.
    pub time_domain: (i64, i64),
    pub palette: PressurePalette,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PressurePaneModel {
    pub scene: Scene,
    pub bars_top: f32,
    pub bars_bottom: f32,
}

fn naive(ts: i64) -> chrono::NaiveDateTime {
    chrono::DateTime::from_timestamp(ts, 0)
        .unwrap_or_default()
        .naive_utc()
}

fn time_scale(o: &PressurePaneOptions) -> TimeScale {
    TimeScale::new(
        naive(o.time_domain.0),
        naive(o.time_domain.1),
        (PANE_MARGIN_LEFT, o.width - PANE_MARGIN_RIGHT),
    )
}

/// Centre x of the bucket starting at `start`.
pub fn bucket_x(p: &UndercutPressure, o: &PressurePaneOptions, start: i64) -> f32 {
    time_scale(o).scale(naive(start + p.bucket_seconds / 2))
}

fn known(b: &PressureBucket) -> bool {
    b.state != PressureState::Unknown
}

pub fn build_undercut_pressure_chart(
    p: &UndercutPressure,
    sales: &[(i64, u32)],
    o: &PressurePaneOptions,
) -> PressurePaneModel {
    let time = time_scale(o);
    let (left, right) = (PANE_MARGIN_LEFT, o.width - PANE_MARGIN_RIGHT);
    let bars_top = RIBBON_TOP + RIBBON_HEIGHT + RIBBON_GAP;
    let bars_bottom = o.height - BOTTOM_PAD;
    let sales_in = |start: i64| -> u32 {
        sales
            .iter()
            .filter(|(ts, _)| *ts >= start && *ts < start + p.bucket_seconds)
            .map(|(_, n)| *n)
            .sum()
    };
    let max = p
        .buckets
        .iter()
        .filter(|b| known(b))
        .map(|b| f64::from((b.trims + b.cuts).max(sales_in(b.start))))
        .fold(p.baseline.unwrap_or(0.0), f64::max)
        .max(1.0);
    let y = LinearScale::new((0.0, max), (bars_bottom, bars_top));
    let edge = |ts: i64| time.scale(naive(ts)).clamp(left, right);

    // Untracked time before the world's coverage starts (the endpoint clamps
    // `from` to the anchor, so no buckets exist there): a flat dim band, no
    // bars, so blank never reads as calm. A never-anchored world with no
    // buckets is untracked across the whole domain.
    let unknown_until = match p.coverage_from {
        Some(c) if c > o.time_domain.0 => Some(p.buckets.first().map_or(c, |b| c.min(b.start))),
        None if p.buckets.is_empty() => Some(o.time_domain.1),
        _ => None,
    };
    let unknown_band = unknown_until
        .map(|end| (left, edge(end)))
        .filter(|(x0, x1)| x1 > x0);
    let mut nodes = Vec::new();
    if let Some((x0, x1)) = unknown_band
        && let Some(d) = rects_path_d(&[(x0, bars_top, x1 - x0, bars_bottom - bars_top)])
    {
        nodes.push(Node::Path {
            d,
            fill: Some(o.palette.unknown.with_alpha(0.35)),
            stroke: None,
        });
    }
    nodes.push(Node::Line {
        x1: left,
        y1: bars_bottom,
        x2: right,
        y2: bars_bottom,
        stroke: Stroke {
            color: o.palette.grid,
            width: 1.0,
            dash: None,
        },
    });
    let mut ribbon: [Vec<(f32, f32, f32, f32)>; 4] = Default::default();
    if let Some((x0, x1)) = unknown_band {
        ribbon[3].push((x0, RIBBON_TOP, (x1 - x0 - 1.0).max(0.5), RIBBON_HEIGHT));
    }
    let (mut cut_rects, mut trim_rects, mut sales_points) = (Vec::new(), Vec::new(), Vec::new());
    for b in &p.buckets {
        let (x0, x1) = (edge(b.start), edge(b.start + p.bucket_seconds));
        if x1 <= x0 {
            continue;
        }
        let lane = match b.state {
            PressureState::War => 0,
            PressureState::Churn => 1,
            PressureState::Calm => 2,
            PressureState::Unknown => 3,
        };
        ribbon[lane].push((x0, RIBBON_TOP, (x1 - x0 - 1.0).max(0.5), RIBBON_HEIGHT));
        if !known(b) {
            continue;
        }
        let width = ((x1 - x0) * 0.8).max(1.0);
        let bx = x0 + (x1 - x0 - width) / 2.0;
        let cut_top = y.scale(f64::from(b.cuts));
        let trim_top = y.scale(f64::from(b.cuts + b.trims));
        if b.cuts > 0 {
            cut_rects.push((bx, cut_top, width, bars_bottom - cut_top));
        }
        if b.trims > 0 {
            trim_rects.push((bx, trim_top, width, cut_top - trim_top));
        }
        sales_points.push(((x0 + x1) / 2.0, y.scale(f64::from(sales_in(b.start)))));
    }
    let lanes = [
        o.palette.war,
        o.palette.churn,
        o.palette.calm,
        o.palette.unknown,
    ];
    for (rects, color) in ribbon.iter().zip(lanes) {
        if let Some(d) = rects_path_d(rects) {
            nodes.push(Node::Path {
                d,
                fill: Some(color),
                stroke: None,
            });
        }
    }
    for (rects, color) in [(&cut_rects, o.palette.cut), (&trim_rects, o.palette.trim)] {
        if let Some(d) = rects_path_d(rects) {
            nodes.push(Node::Path {
                d,
                fill: Some(color),
                stroke: None,
            });
        }
    }
    if let Some(baseline) = p.baseline.filter(|b| *b > 0.0) {
        let by = y.scale(baseline);
        nodes.push(Node::Line {
            x1: left,
            y1: by,
            x2: right,
            y2: by,
            stroke: Stroke {
                color: o.palette.baseline,
                width: 1.5,
                dash: Some((5.0, 3.0)),
            },
        });
    }
    if sales_points.len() >= 2 {
        nodes.push(Node::Polyline {
            points: sales_points,
            stroke: Stroke {
                color: o.palette.sales,
                width: 2.0,
                dash: None,
            },
        });
    }
    PressurePaneModel {
        scene: Scene {
            width: o.width,
            height: o.height,
            background: None,
            font_family: Theme::site().font_family,
            nodes,
        },
        bars_top,
        bars_bottom,
    }
}

/// Six hourly buckets: unknown, churn, war, churn, calm, churn. Shared by
/// this crate's tests/snapshots and `ultros-ui-charts` tests.
#[doc(hidden)]
pub fn tests_fixture() -> UndercutPressure {
    let bucket = |i: i64, trims, cuts, state| PressureBucket {
        start: i * 3600,
        trims,
        cuts,
        sellers: 2,
        floor_open: Some(1000),
        floor_close: Some(990),
        state,
    };
    UndercutPressure {
        world_id: 34,
        from: 0,
        to: 6 * 3600,
        bucket_seconds: 3600,
        coverage_from: Some(3600),
        baseline: Some(3.0),
        buckets: vec![
            bucket(0, 0, 0, PressureState::Unknown),
            bucket(1, 2, 1, PressureState::Churn),
            bucket(2, 1, 6, PressureState::War),
            bucket(3, 3, 0, PressureState::Churn),
            bucket(4, 0, 0, PressureState::Calm),
            bucket(5, 1, 1, PressureState::Churn),
        ],
        wars: vec![],
        summary: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> PressurePaneOptions {
        PressurePaneOptions {
            width: 960.0,
            height: pane_height(960.0),
            time_domain: (0, 6 * 3600),
            palette: PressurePalette::default(),
        }
    }

    #[test]
    fn bucket_x_matches_the_price_chart_time_scale() {
        let plot = 960.0 - PANE_MARGIN_LEFT - PANE_MARGIN_RIGHT;
        let expected = PANE_MARGIN_LEFT + plot * 2.5 / 6.0;
        assert!((bucket_x(&tests_fixture(), &options(), 2 * 3600) - expected).abs() < 0.01);
    }

    #[test]
    fn bars_skip_unknown_and_stay_in_the_plot() {
        let m = build_undercut_pressure_chart(&tests_fixture(), &[], &options());
        let p = PressurePalette::default();
        let bar_paths: Vec<&String> = m
            .scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Path {
                    d, fill: Some(f), ..
                } if *f == p.cut || *f == p.trim => Some(d),
                _ => None,
            })
            .collect();
        // Cut path appears once and trim path once, even though the war colour equals the cut colour.
        assert!(!bar_paths.is_empty());
        for d in bar_paths {
            for rect in d.split('M').filter(|s| !s.is_empty()) {
                let mut nums = rect
                    .split(|c: char| c == ' ' || c.is_ascii_alphabetic())
                    .filter(|s| !s.is_empty());
                let x: f32 = nums.next().unwrap().parse().unwrap();
                let y: f32 = nums.next().unwrap().parse().unwrap();
                assert!(
                    x >= bucket_x(&tests_fixture(), &options(), 3600) - 60.0,
                    "unknown bucket 0 drew a bar: {rect}"
                );
                assert!(
                    x <= 960.0 - PANE_MARGIN_RIGHT && y >= m.bars_top - 0.5 && y <= m.bars_bottom,
                    "{rect}"
                );
            }
        }
    }

    #[test]
    fn sales_line_zero_fills_missing_buckets() {
        let m = build_undercut_pressure_chart(
            &tests_fixture(),
            &[(3600, 4), (5 * 3600, 1)],
            &options(),
        );
        let line = m
            .scene
            .nodes
            .iter()
            .find_map(|n| match n {
                Node::Polyline { points, .. } => Some(points.clone()),
                _ => None,
            })
            .expect("sales line");
        assert_eq!(line.len(), 5, "one point per known bucket");
        assert!(
            (line[1].1 - m.bars_bottom).abs() < 0.01,
            "bucket 2 has no sales → zero"
        );
    }

    /// The fixture with coverage starting two hours into a six-hour domain
    /// and the pre-coverage buckets dropped, as the endpoint returns it.
    fn unknown_prefix_fixture() -> UndercutPressure {
        let mut p = tests_fixture();
        p.coverage_from = Some(2 * 3600);
        p.from = 2 * 3600;
        p.buckets.retain(|b| b.start >= 2 * 3600);
        p
    }

    fn unknown_paths(m: &PressurePaneModel) -> Vec<(&String, Color)> {
        let unknown = PressurePalette::default().unknown;
        m.scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Path {
                    d, fill: Some(f), ..
                } if (f.r, f.g, f.b) == (unknown.r, unknown.g, unknown.b) => Some((d, *f)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn untracked_prefix_draws_a_dim_unknown_band() {
        let m = build_undercut_pressure_chart(&unknown_prefix_fixture(), &[], &options());
        let paths = unknown_paths(&m);
        let ribbon = paths
            .iter()
            .find(|(_, f)| f.a >= 1.0)
            .expect("unknown ribbon cell");
        let lane = paths
            .iter()
            .find(|(_, f)| f.a < 1.0)
            .expect("dim unknown lane band");
        for (d, _) in [ribbon, lane] {
            assert!(
                d.starts_with(&format!("M{PANE_MARGIN_LEFT:.1} ")),
                "band starts at the plot's left edge: {d}"
            );
        }
        // The band ends where coverage starts (2 h of a 6 h domain).
        let plot = 960.0 - PANE_MARGIN_LEFT - PANE_MARGIN_RIGHT;
        let cover_x = PANE_MARGIN_LEFT + plot * 2.0 / 6.0;
        assert!(
            lane.0
                .contains(&format!("{:.1}", cover_x - PANE_MARGIN_LEFT)),
            "lane band spans left edge → coverage: {}",
            lane.0
        );
    }

    #[test]
    fn never_anchored_empty_payload_is_unknown_everywhere() {
        let mut p = tests_fixture();
        p.coverage_from = None;
        p.buckets.clear();
        p.baseline = None;
        let m = build_undercut_pressure_chart(&p, &[], &options());
        assert_eq!(unknown_paths(&m).len(), 2, "ribbon cell + lane band");
    }

    #[test]
    fn covered_domain_draws_no_band() {
        // The fixture's coverage starts inside its own first (unknown) bucket.
        let m = build_undercut_pressure_chart(&tests_fixture(), &[], &options());
        assert!(unknown_paths(&m).iter().all(|(_, f)| f.a >= 1.0));
    }

    #[test]
    fn baseline_is_dashed_and_ribbon_batches_per_state() {
        let m = build_undercut_pressure_chart(&tests_fixture(), &[], &options());
        assert!(
            m.scene
                .nodes
                .iter()
                .any(|n| matches!(n, Node::Line { stroke, .. } if stroke.dash.is_some()))
        );
        // Ribbon: war, churn, calm, unknown → 4 paths; bars: cut, trim → 2 paths.
        let paths = m
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Path { .. }))
            .count();
        assert_eq!(paths, 6);
    }
}
