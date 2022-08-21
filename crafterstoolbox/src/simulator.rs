use std::collections::HashMap;
use xiv_crafting_sim::{Synth, CraftSimulator};
use xiv_crafting_sim::simulator::SimStep;
use xiv_gen::RecipeId;
use crate::crafting_types::CrafterDetails;

pub(crate) struct SimulatorManager {
    recipe: RecipeId,
    simulator: CraftSimulator
}

impl SimulatorManager {


    fn do_tick(&mut self) {

    }
}


fn process_sim_synth(synth: Synth) {
    //let simulator : CraftSimulator = synth.into();

}

