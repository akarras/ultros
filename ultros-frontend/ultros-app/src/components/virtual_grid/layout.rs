use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnFilter {
    pub key: &'static str,
    pub label: String,
    pub numeric: bool,
    pub options: Vec<(&'static str, String)>,
}

impl ColumnFilter {
    pub fn new(key: &'static str, label: String, numeric: bool) -> Self {
        Self {
            key,
            label,
            numeric,
            options: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GridColumn {
    pub id: &'static str,
    pub label: String,
    pub width: f64,
    pub min_width: f64,
    pub max_width: f64,
    pub optional: bool,
    pub visible: bool,
    pub aria_sort: &'static str,
    pub filters: Vec<ColumnFilter>,
}

impl GridColumn {
    pub fn new(id: &'static str, label: String, width: f64, optional: bool, visible: bool) -> Self {
        Self {
            id,
            label,
            width,
            min_width: 60.0,
            max_width: 800.0,
            optional,
            visible,
            aria_sort: "none",
            filters: Vec::new(),
        }
    }

    pub fn clamp(&self, width: f64) -> f64 {
        if width.is_finite() {
            width.clamp(self.min_width, self.max_width)
        } else {
            self.width
        }
    }

    pub fn sorted(mut self, active: bool, ascending: bool) -> Self {
        self.aria_sort = if !active {
            "none"
        } else if ascending {
            "ascending"
        } else {
            "descending"
        };
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridLayout {
    pub v: u8,
    pub order: Vec<String>,
    #[serde(default)]
    pub widths: BTreeMap<String, f64>,
}

impl GridLayout {
    pub fn parse(raw: Option<&str>, columns: &[GridColumn]) -> Self {
        let parsed = raw
            .filter(|s| s.len() <= 16_384)
            .and_then(|s| {
                if let Some(body) = s.strip_prefix("2~") {
                    let (order, widths) = body.split_once('~')?;
                    let tokens: Vec<_> = widths.split('.').filter(|s| !s.is_empty()).collect();
                    if tokens.len() % 2 != 0 {
                        return None;
                    }
                    let widths = tokens
                        .chunks_exact(2)
                        .map(|p| {
                            Some((p[0].to_string(), u32::from_str_radix(p[1], 36).ok()? as f64))
                        })
                        .collect::<Option<BTreeMap<_, _>>>()?;
                    Some(Self {
                        v: 1,
                        order: order
                            .split('.')
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect(),
                        widths,
                    })
                } else {
                    serde_json::from_str::<Self>(s).ok()
                }
            })
            .filter(|s| s.v == 1);
        let mut layout = parsed.unwrap_or(Self {
            v: 1,
            order: Vec::new(),
            widths: BTreeMap::new(),
        });
        let mut seen = HashSet::new();
        layout
            .order
            .retain(|id| columns.iter().any(|c| c.id == id) && seen.insert(id.clone()));
        for column in columns {
            if seen.insert(column.id.to_string()) {
                layout.order.push(column.id.to_string());
            }
        }
        layout.widths.retain(|id, width| {
            if let Some(column) = columns.iter().find(|c| c.id == id) {
                *width = column.clamp(*width);
                true
            } else {
                false
            }
        });
        layout
    }

    #[cfg(test)]
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Only a changed order prefix and non-default widths belong in a URL.
    /// IDs remain stable if a later release adds or removes other columns.
    pub fn compact(&self, columns: &[GridColumn]) -> Option<String> {
        let prefix = (0..=self.order.len())
            .find(|&n| {
                let used: HashSet<_> = self.order[..n].iter().map(String::as_str).collect();
                self.order[n..]
                    .iter()
                    .map(String::as_str)
                    .eq(columns.iter().map(|c| c.id).filter(|id| !used.contains(id)))
            })
            .unwrap_or(self.order.len());
        let widths = self
            .widths
            .iter()
            .filter_map(|(id, width)| {
                let column = columns.iter().find(|c| c.id == id)?;
                let width = column.clamp(*width).round() as u32;
                (width != column.width.round() as u32).then(|| format!("{id}.{}", base36(width)))
            })
            .collect::<Vec<_>>()
            .join(".");
        if prefix == 0 && widths.is_empty() {
            return None;
        }
        Some(format!("2~{}~{}", self.order[..prefix].join("."), widths))
    }

    pub fn move_to(&mut self, id: &str, target: &str, after: bool) {
        if id == target || !self.order.iter().any(|s| s == target) {
            return;
        }
        if let Some(index) = self.order.iter().position(|s| s == id) {
            let id = self.order.remove(index);
            let index = self.order.iter().position(|s| s == target).unwrap_or(0);
            self.order.insert(index + usize::from(after), id);
        }
    }

    pub fn columns(&self, definitions: &[GridColumn]) -> Vec<PlacedColumn> {
        let mut left = 0.0;
        self.order
            .iter()
            .filter_map(|id| {
                let column = definitions.iter().find(|c| c.id == id && c.visible)?;
                let width = column.clamp(*self.widths.get(id).unwrap_or(&column.width));
                let placed = PlacedColumn {
                    column: column.clone(),
                    left,
                    width,
                };
                left += width;
                Some(placed)
            })
            .collect()
    }
}

fn base36(mut value: u32) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(value % 36, 36).unwrap());
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.into_iter().rev().collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedColumn {
    pub column: GridColumn,
    pub left: f64,
    pub width: f64,
}

pub fn row_range(
    top: f64,
    height: f64,
    row_height: f64,
    count: usize,
    overscan: usize,
) -> (usize, usize) {
    let start = (top.max(0.0) / row_height).floor() as usize;
    let end = ((top.max(0.0) + height.max(0.0)) / row_height).ceil() as usize;
    (
        start.saturating_sub(overscan).min(count),
        end.saturating_add(overscan).min(count),
    )
}

pub fn column_range(columns: &[PlacedColumn], left: f64, width: f64) -> (usize, usize) {
    let start = columns
        .partition_point(|c| c.left + c.width <= left)
        .saturating_sub(1);
    let end = columns
        .partition_point(|c| c.left < left + width)
        .saturating_add(1)
        .min(columns.len());
    (start.min(end), end)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn columns() -> Vec<GridColumn> {
        vec![
            GridColumn::new("item", "Item".into(), 240.0, false, true),
            GridColumn::new("profit", "Profit".into(), 120.0, false, true),
            GridColumn::new("trend", "Trend".into(), 100.0, true, false),
        ]
    }
    #[test]
    fn old_and_invalid_links_use_defaults() {
        let default = GridLayout::parse(None, &columns());
        for raw in ["broken", r#"{"v":2,"order":["profit"]}"#] {
            assert_eq!(GridLayout::parse(Some(raw), &columns()), default);
        }
        assert_eq!(
            GridLayout::parse(Some(&"x".repeat(20_000)), &columns()),
            default
        );
    }
    #[test]
    fn normalizes_duplicates_unknown_ids_and_widths() {
        let state = GridLayout::parse(
            Some(
                r#"{"v":1,"order":["profit","gone","profit"],"widths":{"item":2,"profit":9999,"gone":55}}"#,
            ),
            &columns(),
        );
        assert_eq!(state.order, ["profit", "item", "trend"]);
        assert_eq!(state.widths.len(), 2);
        assert_eq!(
            state
                .columns(&columns())
                .iter()
                .map(|c| (c.left, c.width))
                .collect::<Vec<_>>(),
            [(0.0, 800.0), (800.0, 60.0)]
        );
        assert_eq!(GridLayout::parse(Some(&state.encode()), &columns()), state);
    }
    #[test]
    fn insertion_and_hiding_preserve_geometry() {
        let mut defs = columns();
        let mut state = GridLayout::parse(None, &defs);
        state.move_to("trend", "item", false);
        defs[2].visible = true;
        assert_eq!(
            state
                .columns(&defs)
                .iter()
                .map(|c| c.column.id)
                .collect::<Vec<_>>(),
            ["trend", "item", "profit"]
        );
        state.move_to("trend", "profit", true);
        assert_eq!(state.order, ["item", "profit", "trend"]);
        state.move_to("item", "item", true);
        assert_eq!(state.order, ["item", "profit", "trend"]);
    }
    #[test]
    fn compact_layout_only_stores_changes_and_survives_registry_growth() {
        let defs = columns();
        let mut layout = GridLayout::parse(None, &defs);
        assert_eq!(layout.compact(&defs), None);
        layout.widths.insert("profit".into(), 100.0);
        assert_eq!(layout.compact(&defs).as_deref(), Some("2~~profit.2s"));
        assert_eq!(
            GridLayout::parse(layout.compact(&defs).as_deref(), &defs),
            layout
        );
        layout.move_to("trend", "item", false);
        let compact = layout.compact(&defs).unwrap();
        assert_eq!(compact, "2~trend~profit.2s");
        let mut newer = defs.clone();
        newer.insert(1, GridColumn::new("new", "New".into(), 100.0, true, false));
        let restored = GridLayout::parse(Some(&compact), &newer);
        assert_eq!(restored.order, ["trend", "item", "new", "profit"]);
        assert_eq!(restored.widths["profit"], 100.0);
        assert!(compact.len() * 3 < layout.encode().len());
        layout.widths.insert("profit".into(), 120.0);
        layout.move_to("trend", "profit", true);
        assert_eq!(layout.compact(&defs), None);
    }
    #[test]
    fn both_axes_are_bounded_and_last_cells_reachable() {
        let defs: Vec<_> = (0..1000)
            .map(|_| GridColumn::new("test", "Test".into(), 100.0, false, true))
            .collect();
        let placed: Vec<_> = defs
            .into_iter()
            .enumerate()
            .map(|(i, column)| PlacedColumn {
                column,
                left: i as f64 * 100.0,
                width: 100.0,
            })
            .collect();
        assert_eq!(column_range(&placed, 99_200.0, 800.0), (991, 1000));
        assert_eq!(row_range(399_400.0, 600.0, 40.0, 10_000, 4), (9981, 10_000));
        assert_eq!(row_range(0.0, 600.0, 40.0, 0, 4), (0, 0));
        assert_eq!(column_range(&[], 0.0, 800.0), (0, 0));
    }
}
