use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};
use egui::plot::{BoxElem, BoxPlot, BoxSpread, Legend, Line, Plot, PlotPoints};
use egui::{Color32, Stroke, Ui};
use itertools::Itertools;
use std::collections::HashMap;
use std::ops::{Add, Div};
use universalis::{HistoryEntry, HistorySingleView, HistoryView};
use xiv_gen::ItemId;

/// Provides formatting along an x axis
#[derive(Debug, Clone)]
struct DateAxisFormatter {
    start_date: DateTime<Local>,
    end_date: DateTime<Local>,
    day_size: f64,
}

impl DateAxisFormatter {
    fn interpolate(&self, date: DateTime<Local>) -> f64 {
        let date = date.clamp(self.start_date, self.end_date);
        let total_duration = self.end_date - self.start_date;
        let pos = date - self.start_date;
        // Value between 0 - 1 that represents how far into the axis this is
        let base_ms = pos.num_milliseconds() as f64 / total_duration.num_milliseconds() as f64;
        //let total_ms = total_duration.num_milliseconds() as f64;
        let scaling_factor = (total_duration.num_minutes() as f64 / 24.0 / 60.0) * self.day_size;
        base_ms * scaling_factor
    }

    fn interpolate_x_value(&self, interp_x: f64) -> DateTime<Local> {
        let total_duration = self.end_date - self.start_date;

        let scale = total_duration.num_minutes() as f64 / 60.0 / 24.0;
        let dur_ms = interp_x * scale / self.day_size;
        self.start_date + Duration::milliseconds(dur_ms as i64)
    }
}

struct HistoryLine {
    item_id: ItemId,
    plot_points: Vec<[f64; 2]>,
    date_axis: DateAxisFormatter,
}

struct HistoryData<'a>(&'a [&'a HistoryEntry], &'a DateAxisFormatter);

impl<'a> Into<Vec<[f64; 2]>> for HistoryData<'a> {
    fn into(self) -> Vec<[f64; 2]> {
        self.0
            .iter()
            .map(|m| [self.1.interpolate(m.timestamp), m.price_per_unit as f64])
            .collect()
    }
}

impl TryFrom<&HistorySingleView> for HistoryLine {
    type Error = anyhow::Error;

    fn try_from(view: &HistorySingleView) -> Result<Self, Self::Error> {
        let entries: Vec<_> = view
            .entries
            .iter()
            .sorted_by(|a, b| a.timestamp.cmp(&b.timestamp))
            .collect();
        let (start_date, end_date) = entries
            .iter()
            .map(|a| a.timestamp)
            .minmax()
            .into_option()
            .ok_or(anyhow::Error::msg("No valid data"))?;
        let formatter = DateAxisFormatter {
            start_date,
            end_date,
            day_size: 24.0,
        };
        let history_data = HistoryData(entries.as_slice(), &formatter);

        Ok(Self {
            item_id: ItemId(view.item_id as i32),
            plot_points: history_data.into(),
            date_axis: formatter,
        })
    }
}

pub struct CandleStickHistoryPlot {
    date_formatter: DateAxisFormatter,
    item_ids: Vec<ItemId>,
    plots: Vec<BoxPlot>,
}

impl CandleStickHistoryPlot {
    pub fn draw_graph(self, ui: &mut Ui) {
        let formatter = self.date_formatter;
        Plot::new(format!(
            "CandleStick:{}",
            self.item_ids
                .iter()
                .map(|m| m.0.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ))
        .x_axis_formatter(move |x, range| {
            formatter
                .interpolate_x_value(x)
                .format("%Y-%m-%d\n%H:%M")
                .to_string()
        })
        .y_axis_formatter(|y, range| format!("{y} gil"))
        .legend(Legend::default())
        .height(300.0)
        .show(ui, |ui| {
            for b in self.plots {
                ui.box_plot(b);
            }
        });
    }
}

struct DataSummary<D> {
    quartiles: (D, D, D),
    minimum: D,
    maximum: D,
}

trait Half {
    fn half(self) -> Self;
}

impl Half for u64 {
    fn half(self) -> Self {
        self / 2
    }
}

impl<D> DataSummary<D>
where
    D: Copy + Add + Div + Ord + Half + Add<Output = D>,
    <D as Add>::Output: Half,
{
    fn median(data: &[D]) -> Option<(D, &[D], &[D])> {
        let length = data.len();
        let mid_point = length / 2;
        if length % 2 == 0 {
            let left_hand_side = &data[..=mid_point];
            let right_hand_side = &data[mid_point + 1..];
            let last = *left_hand_side.last()?;
            let first = *right_hand_side.first()?;
            Some(((first + last).half(), left_hand_side, right_hand_side))
        } else {
            let median = data.get(mid_point)?;
            let lhs = &data[..=mid_point];
            let rhs = &data[mid_point..];
            Some((*median, lhs, rhs))
        }
    }

    fn from_slice(data: &mut [D]) -> Option<Self> {
        data.sort();
        let (median, lhs, rhs) = Self::median(data)?;
        let (first_quartile, _, _) = Self::median(lhs)?;
        let (second_quartile, _, _) = Self::median(rhs)?;
        Some(Self {
            quartiles: (first_quartile, median, second_quartile),
            minimum: *data.first()?,
            maximum: *data.last()?,
        })
    }
}

#[derive(PartialOrd, Eq, PartialEq, Debug, Hash)]
enum DateGroupByOptions {
    // yyyy  mm   dd
    Day(i32, u32, u32),
    // yy  mm dd hh
    Hour(i32, u32, u32, u32),
}

impl From<DateGroupByOptions> for DateTime<Local> {
    fn from(val: DateGroupByOptions) -> Self {
        match val {
            DateGroupByOptions::Day(year, month, day) => {
                Local.ymd(year, month, day).and_hms(0, 0, 0)
            }
            DateGroupByOptions::Hour(year, month, day, hour) => {
                Local.ymd(year, month, day).and_hms(hour, 0, 0)
            }
        }
    }
}

fn create_box_plot_elem<'a>(
    time: DateGroupByOptions,
    mut prices: Vec<u64>,
    date_formatter: &DateAxisFormatter,
    color: &Color32,
    parent_str: &str,
) -> Option<BoxElem> {
    let time = time.into();
    DataSummary::from_slice(prices.as_mut_slice()).map(|summary| {
        BoxElem::new(
            date_formatter.interpolate(time),
            BoxSpread::new(
                summary.minimum as f64,
                summary.quartiles.0 as f64,
                summary.quartiles.1 as f64,
                summary.quartiles.2 as f64,
                summary.maximum as f64,
            ),
        )
        .whisker_width(1.25)
        .box_width(0.75)
        .vertical()
        .name(format!("{parent_str}\n{}", time.format("%Y-%m-%d %H:%M")))
        .stroke(Stroke::new(0.4, color.clone()))
        .fill(color.linear_multiply(0.05))
    })
}

impl CandleStickHistoryPlot {
    pub fn from_custom_iter<'a, Iter, Inner>(i: Iter) -> anyhow::Result<Self>
    where
        Iter: Iterator<Item = &'a (Inner, Color32, String)> + Clone,
        Inner: IntoIterator<Item = &'a HistoryEntry> + 'a + Clone,
    {
        let (start_date, end_date) = i
            .clone()
            .cloned()
            .map(|(i, _, _)| i.into_iter().map(|h| h.timestamp))
            .flatten()
            .minmax()
            .into_option()
            .ok_or(anyhow::Error::msg("No data to build x-axis"))?;
        let duration = end_date - start_date;
        let is_month = duration.num_days() > 2;
        let date_formatter = DateAxisFormatter {
            start_date,
            end_date,
            day_size: is_month.then(|| 2).unwrap_or(48) as f64,
        };
        let plots: Vec<_> = i
            .into_iter()
            .cloned()
            .map(|(i, color, name)| {
                let hash_map: HashMap<_, Vec<_>> = i
                    .into_iter()
                    .group_by(|e| {
                        if is_month {
                            DateGroupByOptions::Day(
                                e.timestamp.year(),
                                e.timestamp.month(),
                                e.timestamp.day(),
                            )
                        } else {
                            DateGroupByOptions::Hour(
                                e.timestamp.year(),
                                e.timestamp.month(),
                                e.timestamp.day(),
                                e.timestamp.hour(),
                            )
                        }
                    })
                    .into_iter()
                    .map(|(i, g)| (i, g.map(|m| m.price_per_unit).collect()))
                    .collect();
                BoxPlot::new(
                    hash_map
                        .into_iter()
                        .flat_map(|(time, data)| {
                            create_box_plot_elem(time, data, &date_formatter, &color, &name)
                        })
                        .collect(),
                )
                .color(color.clone())
                .name(name)
            })
            .collect();
        Ok(Self {
            date_formatter,
            item_ids: vec![],
            plots,
        })
    }
}

pub struct HistoryPlot {
    lines: Vec<HistoryLine>,
    date_formatter: DateAxisFormatter,
}

impl TryFrom<&HistoryView> for HistoryPlot {
    type Error = anyhow::Error;

    fn try_from(value: &HistoryView) -> Result<Self, Self::Error> {
        match value {
            HistoryView::SingleView(single_view) => {
                let line: HistoryLine = single_view.try_into()?;
                let date_formatter = line.date_axis.clone();
                Ok(Self {
                    lines: vec![line],
                    date_formatter,
                })
            }
            HistoryView::MultiView(_) => {
                unimplemented!("multiview plot not supported yet");
            }
        }
    }
}

impl HistoryPlot {
    pub(crate) fn draw(&self, ui: &mut Ui, height: f32) {
        let date_axis_format_data = self.date_formatter.clone();
        let label_date_format_data = self.date_formatter.clone();
        Plot::new(format!(
            "PLOT: {}",
            self.lines
                .iter()
                .map(|m| m.item_id.0.to_string())
                .collect::<Vec<String>>()
                .join(",")
        ))
        .x_axis_formatter(move |x, range| {
            let num_days = x / date_axis_format_data.day_size;
            let day = date_axis_format_data.start_date
                + Duration::seconds((num_days * 24.0 * 60.0 * 60.0) as i64);
            day.format("%Y-%m-%d").to_string()
        })
        .label_formatter(move |x, point| {
            let day = label_date_format_data.interpolate_x_value(point.x);
            let gil = point.y;
            let x_format = day.format("%Y-%m-%d \n%H:%M");
            format!("day: {x_format}\ngil: {gil}")
        })
        .height(height)
        .show(ui, |ui| {
            for line in &self.lines {
                ui.line(Line::new(PlotPoints::new(line.plot_points.clone())));
            }
        });
    }
}
