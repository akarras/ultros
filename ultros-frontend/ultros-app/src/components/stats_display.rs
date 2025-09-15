use std::collections::HashMap;

use super::tooltip::*;
use leptos::prelude::*;
use xiv_gen::{BaseParam, Item, ItemId};

struct ParamData {
    base_param: &'static BaseParam,
    normal_value: i16,
    special_value: Option<i16>,
}

struct ParamIterator {
    index: u8,
    item: &'static Item,
}

impl ParamIterator {
    fn new(item: &'static Item) -> Self {
        Self { index: 0, item }
    }
}

impl Iterator for ParamIterator {
    type Item = (&'static BaseParam, bool, i16);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.item;
        loop {
            let (param, value) = match self.index {
                0 => (item.base_param_0, item.base_param_value_0),
                1 => (item.base_param_1, item.base_param_value_1),
                2 => (item.base_param_2, item.base_param_value_2),
                3 => (item.base_param_3, item.base_param_value_3),
                4 => (item.base_param_4, item.base_param_value_4),
                5 => (item.base_param_5, item.base_param_value_5),
                6 => (item.base_param_special_0, item.base_param_value_special_0),
                7 => (item.base_param_special_1, item.base_param_value_special_1),
                8 => (item.base_param_special_2, item.base_param_value_special_2),
                9 => (item.base_param_special_3, item.base_param_value_special_3),
                10 => (item.base_param_special_4, item.base_param_value_special_4),
                11 => (item.base_param_special_5, item.base_param_value_special_5),
                _ => return None,
            };
            self.index += 1;
            if let Some(base_param) = xiv_gen_db::data().base_params.get(&param) {
                return Some((base_param, self.index > 5, value));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(10))
    }
}

fn get_param_data_for_item(item: ItemId) -> Option<Vec<ParamData>> {
    let item = xiv_gen_db::data().items.get(&item)?;
    let mut params = ParamIterator::new(item)
        .map(|(param, hq, value)| ((param.key_id, hq), (param, value)))
        // .filter(|((_, _), (_, v))| *v > 0)
        .fold(HashMap::new(), |mut acc, ((bp, hq), (param, value))| {
            let entry = acc.entry(bp).or_insert(ParamData {
                base_param: param,
                normal_value: 0,
                special_value: None,
            });
            if hq {
                entry.special_value = Some(value);
            } else {
                entry.normal_value = value;
            }
            acc
        })
        .into_values()
        .collect::<Vec<_>>();

    params.sort_by_key(|param| param.base_param.order_priority);
    Some(params)
}

#[component]
fn ParamView(data: ParamData) -> impl IntoView {
    view! {
        <div>
            <Tooltip tooltip_text=data.base_param.description.as_str()>
                <span class="w-48">{data.base_param.name.as_str()}</span>
                "  "
                {data.normal_value}
                {data
                    .special_value
                    .map(|special| {
                        view! {
                            " hq: "
                            {data.normal_value + special}
                        }
                    })}

            </Tooltip>
        </div>
    }
}

#[component]
pub(crate) fn ItemStats(item_id: ItemId) -> impl IntoView {
    let params = get_param_data_for_item(item_id);
    params
        .map(|p| {
            p.into_iter()
                .map(|p| view! { <ParamView data=p /> })
                .collect::<Vec<_>>()
        })
        .into_any()
}

