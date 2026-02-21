use std::collections::HashMap;

use super::tooltip::*;
use leptos::prelude::*;
use xiv_gen::{BaseParam, BaseParamId, Item, ItemId};

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
            let idx = self.index;
            let (param, value): (u8, i16) = if idx < 6 {
                let idx = idx as usize;
                (item.base_param[idx], item.base_param_value[idx])
            } else if idx < 12 {
                let idx = idx as usize - 6;
                (
                    item.base_param_special[idx],
                    item.base_param_value_special[idx],
                )
            } else {
                return None;
            };
            self.index += 1;
            if let Some(base_param) = xiv_gen_db::data()
                .base_params
                .get(&BaseParamId(param as i32))
            {
                return Some((base_param, self.index > 6, value));
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
        <div class="w-full">
            <Tooltip
                class="w-full"
                tooltip_text=data.base_param.description.as_str()
            >
                <div class="flex justify-between items-center w-full gap-x-2">
                    <span class="text-brand-300 truncate">{data.base_param.name.as_str()}</span>
                    <div class="flex items-center gap-x-2 flex-shrink-0">
                        <span class="font-medium text-brand-100">{data.normal_value}</span>
                        {data
                            .special_value
                            .map(|special| {
                                view! {
                                    <span class="text-brand-400 text-xs whitespace-nowrap">
                                        "(HQ: "
                                        {data.normal_value + special}
                                        ")"
                                    </span>
                                }
                            })}
                    </div>
                </div>
            </Tooltip>
        </div>
    }
}

#[component]
pub(crate) fn ItemStats(item_id: ItemId) -> impl IntoView {
    let params = get_param_data_for_item(item_id);
    params
        .map(|p| {
            view! {
                <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-x-8 gap-y-2 w-full">
                    {p.into_iter()
                        .map(|p| view! { <ParamView data=p /> })
                        .collect_view()}
                </div>
            }
        })
        .into_any()
}
