use crate::app::WindowsList;
use crate::{AppRx, AppTx, CraftersToolbox};
use egui::{ScrollArea, Ui};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};
use xiv_gen::{Data, Item, ItemId};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ItemPanel {
    #[serde(skip)]
    items: Vec<(ItemId, String, String)>,
    #[serde(skip)]
    item_query: Vec<(ItemId, String, String)>,
    item_query_string: String,
}

impl ItemPanel {
    pub fn new() -> Self {
        Self {
            items: Self::get_item_data(),
            item_query: vec![],
            item_query_string: "".to_string(),
        }
    }

    pub fn draw(
        &mut self,
        ui: &mut Ui,
        universalis_datacenter: &str,
        windows_list: &mut WindowsList,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        game_data: &xiv_gen::Data,
    ) {
        self.check_invalid();
        ui.heading("Item search");
        if ui
            .text_edit_singleline(&mut self.item_query_string)
            .changed()
        {
            self.run_query();
        }
        let item_query = &self.item_query;
        ScrollArea::vertical().show_rows(ui, 15.0, item_query.len(), |ui, range| {
            for i in range {
                let (id, item_name, category_name) = &item_query[i];
                ui.label(item_name.as_str());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(), |ui| {
                        ui.scope(|ui| {
                            // let already_open =
                            //    crafts_list.windows.iter().any(|list| *id == list.recipe_id);
                            //ui.set_enabled(!already_open);
                            if ui.button("💲").clicked() {
                                windows_list.add_item(
                                    *id,
                                    network_channel,
                                    universalis_datacenter.to_string(),
                                );
                            }
                        });
                        //if ui.button("⚒").clicked() {
                        //    println!("todo implement");
                        //}
                        ui.label(category_name);
                    });
                });
            }
        });
    }

    fn check_invalid(&mut self) {
        if self.items.is_empty() {
            self.items = Self::get_item_data();
            self.run_query();
        }
    }

    fn run_query(&mut self) {
        let lower = self.item_query_string.to_lowercase();
        self.item_query = self
            .items
            .iter()
            .filter(|(_, name, _)| name.to_lowercase().contains(&lower))
            .cloned()
            .collect();
    }

    fn get_item_data() -> Vec<(ItemId, String, String)> {
        let data = CraftersToolbox::decompress_data();
        let items = data.get_items();
        let ui_category_ids = data.get_item_ui_categorys();

        items
            .iter()
            .filter(|(_, item)| !item.get_is_untradable())
            .map(|(item_id, item)| {
                (
                    *item_id,
                    item.get_name(),
                    ui_category_ids
                        .get(&item.get_item_ui_category())
                        .map(|category| category.get_name())
                        .unwrap_or_default(),
                )
            })
            .collect()
    }
}
