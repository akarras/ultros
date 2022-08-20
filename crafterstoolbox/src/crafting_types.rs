use egui::Widget;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(default)]
pub(crate) struct CrafterDetails {
    cp: u32,
    control: u32,
    craftsmanship: u32,
    level: u32,
}

pub(crate) fn create_crafter_menu(ui: &mut egui::Ui, crafter_details: &mut CrafterDetails) {
    let values = [
        ("craftsmanship: ", &mut crafter_details.craftsmanship),
        ("control: ", &mut crafter_details.control),
        ("cp: ", &mut crafter_details.cp),
        ("level: ", &mut crafter_details.level),
    ];
    for (label, value) in values {
        ui.label(label);
        egui::DragValue::new(value).ui(ui);
    }
}

#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(default)]
pub(crate) struct Crafters {
    pub(crate) carpenter: CrafterDetails,
    pub(crate) blacksmith: CrafterDetails,
    pub(crate) armorer: CrafterDetails,
    pub(crate) goldsmith: CrafterDetails,
    pub(crate) leatherworker: CrafterDetails,
    pub(crate) weaver: CrafterDetails,
    pub(crate) alchemist: CrafterDetails,
    pub(crate) culinarian: CrafterDetails,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub enum CraftJob {
    Carpenter,
    Blacksmith,
    Armorer,
    Goldsmith,
    Leatherworker,
    Weaver,
    Alchemist,
    Culinarian,
}

impl Display for CraftJob {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                CraftJob::Carpenter => "CRP",
                CraftJob::Blacksmith => "BSM",
                CraftJob::Armorer => "ARM",
                CraftJob::Goldsmith => "GSM",
                CraftJob::Leatherworker => "LTW",
                CraftJob::Weaver => "WVR",
                CraftJob::Alchemist => "ALC",
                CraftJob::Culinarian => "CUL",
            }
        )
    }
}
