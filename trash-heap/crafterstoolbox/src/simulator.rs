use crate::crafting_types::CrafterDetails;
use crate::{AppRx, AppTx};
use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};
use xiv_crafting_sim::simulator::SimStep;
use xiv_crafting_sim::{CraftSimulator, Synth};
use xiv_gen::RecipeId;

pub(crate) struct SimulatorManager {
    // recipe: RecipeId,
    simulator: CraftSimulator,
}

impl SimulatorManager {
    fn do_tick(&mut self) {}
}

impl SimulatorManager {
    fn new(simulator: CraftSimulator) -> (Self, Sender<AppRx>, Receiver<AppTx>) {
        SimulatorManager { simulator }
    }
}

fn process_sim_synth(synth_str: &str) {
    let synth: Synth = serde_json::from_str(synth_str).unwrap();

    let simulator: CraftSimulator = synth.into();
    SimulatorManager::new(simulator);
}
