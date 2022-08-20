pub(crate) mod item_panel;
mod recipe_search;

use crate::app::WindowsList;
use crate::sidepanel::item_panel::ItemPanel;
use crate::sidepanel::recipe_search::RecipeSearchPanel;
use crate::{AppRx, AppTx, CraftersToolbox};
use egui::Ui;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum SidePanel {
    ItemLookup(ItemPanel),
    RecipeLookup(RecipeSearchPanel),
}

impl SidePanel {
    pub(crate) fn draw_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| match self {
            SidePanel::ItemLookup(_) => {
                if ui.button("recipe lookup").clicked() {
                    *self = SidePanel::RecipeLookup(RecipeSearchPanel::new())
                }
            }
            SidePanel::RecipeLookup(_) => {
                if ui.button("item lookup").clicked() {
                    *self = SidePanel::ItemLookup(ItemPanel::new())
                }
            }
        });
    }

    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        universalis_datacenter: &str,
        windows: &mut WindowsList,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        game_data: &xiv_gen::Data,
    ) {
        self.draw_tab(ui);
        match self {
            SidePanel::ItemLookup(i) => i.draw(
                ui,
                universalis_datacenter,
                windows,
                network_channel,
                game_data,
            ),
            SidePanel::RecipeLookup(r) => r.draw(
                ui,
                universalis_datacenter,
                windows,
                network_channel,
                game_data,
            ),
        }
    }
}
