//! Batch and whole-stack planning, independent of the analyzer's unit estimates.
//! All quantities/costs are integers. Missing supply is never treated as free.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub id: i32,
    pub output: i32,
    pub yield_amount: i64,
    pub ingredients: Vec<(i32, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct Material {
    pub item: i32,
    pub needed: i64,
    pub owned: i64,
    pub crafts: i64,
    pub surplus: i64,
    pub depth: usize,
    pub recipe: Option<i32>,
}

impl Material {
    pub fn remaining(&self) -> i64 {
        self.needed - self.owned
    }
}

/// Desired output count for a single root recipe in a multi-recipe project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct TargetDemand {
    pub recipe_id: i32,
    pub count: i64,
}

/// Bounds that keep cyclic, deep, or oversized crafting DAGs predictable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct ExpandLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_targets: usize,
}

impl ExpandLimits {
    pub const SINGLE_RECIPE: Self = Self {
        max_depth: 12,
        max_nodes: 128,
        max_targets: 1,
    };
}

impl Default for ExpandLimits {
    fn default() -> Self {
        Self {
            max_depth: 12,
            max_nodes: 512,
            max_targets: 64,
        }
    }
}

/// Attribution showing how many units of an intermediate or material are
/// consumed directly by a root target or parent intermediate.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct StepContribution {
    pub target_recipe_id: i32,
    pub target_item_id: i32,
    pub units_used: i64,
}

/// Expanded multi-recipe project broken into targets, bottom-up sub-crafts,
/// and consolidated buyable leaf materials.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct Expansion {
    /// Buyable leaves in deterministic `item` order (`recipe.is_none()`).
    pub materials: Vec<Material>,
    /// Crafted intermediates in bottom-up execution order (deepest sub-crafts first).
    pub steps: Vec<Material>,
    /// Root targets (`depth = 0`, `crafts` and `surplus` computed per target).
    pub targets: Vec<Material>,
    /// Maps each intermediate or leaf item to its direct consumers in deterministic order.
    pub consumers: BTreeMap<i32, Vec<StepContribution>>,
}

/// Readiness of a crafting step or root target against acquired leaves,
/// on-hand stock, and completed child sub-crafts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepReadiness {
    Done,
    Ready { craftable_now: i64 },
    NeedMaterials { missing_kinds: usize },
    Buying,
}

/// Result of bottom-up Craft-vs-Buy optimization over intermediate sub-crafts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct OptimizePlan {
    /// Intermediate `item_id` -> `Some(recipe_id)` for Craft, `None` for Buy.
    pub choices: BTreeMap<i32, Option<i32>>,
    pub total_cost: i64,
    pub total_missing: i64,
    pub craft_all_cost: i64,
    pub craft_all_missing: i64,
    pub buy_all_cost: i64,
    pub buy_all_missing: i64,
    pub savings_vs_current: i64,
    pub flipped_items: Vec<i32>,
}

/// Summary comparing consolidated crafting shopping cost vs buying finished targets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct ProjectCostSummary {
    pub craft_shopping_cost: Option<i64>,
    pub craft_missing_units: i64,
    pub buy_finished_cost: Option<i64>,
    pub buy_finished_missing_units: i64,
    pub buy_finished_priced_targets: usize,
    pub target_count: usize,
    /// Positive means crafting saves gil over buying finished items.
    pub savings_vs_buying_finished: Option<i64>,
}

/// Expand multiple root recipes into consolidated targets, bottom-up sub-craft
/// steps, and deduplicated leaf materials.
///
/// Batch yield rounding (`ceil((needed - owned) / yield)`) and `owned`
/// (`on_hand`) deductions happen once per shared intermediate node across all
/// targets rather than separately per root recipe.
pub fn expand_many(
    targets: &[TargetDemand],
    recipes: &BTreeMap<i32, Recipe>,
    choices: &BTreeMap<i32, i32>,
    owned: &BTreeMap<i32, i64>,
    excluded: &BTreeSet<i32>,
    limits: ExpandLimits,
) -> Result<Expansion, String> {
    let active_targets: BTreeMap<i32, i64> = {
        let mut combined = BTreeMap::new();
        for target in targets {
            if target.count <= 0 {
                continue;
            }
            let entry = combined.entry(target.recipe_id).or_insert(0_i64);
            *entry = entry.saturating_add(target.count).clamp(1, 9999);
        }
        combined
    };
    if active_targets.len() > limits.max_targets {
        return Err(
            "This crafting project has too many targets. Remove some targets to simplify it."
                .into(),
        );
    }
    if active_targets.is_empty() {
        return Ok(Expansion::default());
    }

    // Validate root target recipes upfront and seed depth-1 demand pools.
    let mut root_recipes = Vec::with_capacity(active_targets.len());
    let mut root_items = BTreeSet::new();
    let mut root_demands: BTreeMap<i32, i64> = BTreeMap::new();
    let mut consumers_map: BTreeMap<i32, BTreeMap<(i32, i32), i64>> = BTreeMap::new();
    let mut target_materials = Vec::with_capacity(active_targets.len());

    for (&recipe_id, &count) in &active_targets {
        let recipe = recipes.get(&recipe_id).ok_or("Recipe unavailable")?;
        if recipe.yield_amount <= 0 {
            return Err("Invalid recipe choice".into());
        }
        root_items.insert(recipe.output);
        root_recipes.push((recipe_id, recipe, count));
    }

    // Walk the intermediate DAG to check bounds, cycles (including edges back
    // into any root output), and establish DFS postorder for parent-first vs
    // bottom-up evaluation.
    struct Walk<'a> {
        recipes: &'a BTreeMap<i32, Recipe>,
        choices: &'a BTreeMap<i32, i32>,
        excluded: &'a BTreeSet<i32>,
        limits: ExpandLimits,
        active: BTreeSet<i32>,
        seen: BTreeSet<i32>,
        order: Vec<(i32, usize)>,
    }
    impl Walk<'_> {
        fn visit(&mut self, item: i32, depth: usize) -> Result<(), String> {
            if self.active.contains(&item) {
                return Err(
                    "These craft choices form a cycle. Choose Buy for one of the ingredients."
                        .into(),
                );
            }
            if self.seen.contains(&item) {
                return Ok(());
            }
            if depth > self.limits.max_depth || self.seen.len() >= self.limits.max_nodes {
                return Err(
                    "This crafting plan is too large. Buy some intermediates to simplify it."
                        .into(),
                );
            }
            self.active.insert(item);
            if let Some(id) = self.choices.get(&item) {
                let recipe = self.recipes.get(id).ok_or("Recipe unavailable")?;
                if recipe.output != item || recipe.yield_amount <= 0 {
                    return Err("Invalid recipe choice".into());
                }
                for (child, amount) in &recipe.ingredients {
                    if *amount > 0 && !self.excluded.contains(child) {
                        self.visit(*child, depth + 1)?;
                    }
                }
            }
            self.active.remove(&item);
            self.seen.insert(item);
            self.order.push((item, depth));
            Ok(())
        }
    }

    for (recipe_id, recipe, count) in &root_recipes {
        let crafts = (count + recipe.yield_amount - 1) / recipe.yield_amount;
        let surplus = crafts
            .checked_mul(recipe.yield_amount)
            .and_then(|total| total.checked_sub(*count))
            .ok_or("Quantity too large")?;
        target_materials.push(Material {
            item: recipe.output,
            needed: *count,
            owned: 0,
            crafts,
            surplus,
            depth: 0,
            recipe: Some(*recipe_id),
        });
        for (child, amount) in &recipe.ingredients {
            if *amount <= 0 || excluded.contains(child) {
                continue;
            }
            if root_items.contains(child) && choices.contains_key(child) {
                // Allow a root target item to also appear as an intermediate,
                // as long as expanding that intermediate cannot loop back to
                // the same root recipe output (checked in Walk below).
            }
            let added = crafts.checked_mul(*amount).ok_or("Quantity too large")?;
            let demand = root_demands.entry(*child).or_insert(0_i64);
            *demand = demand
                .checked_add(added)
                .filter(|n| *n <= 1_000_000_000)
                .ok_or("Quantity too large")?;
            *consumers_map
                .entry(*child)
                .or_default()
                .entry((*recipe_id, recipe.output))
                .or_insert(0_i64) += added;
        }
    }

    let mut walk = Walk {
        recipes,
        choices,
        excluded,
        limits,
        active: BTreeSet::new(),
        seen: BTreeSet::new(),
        order: Vec::new(),
    };
    for &child in root_demands.keys() {
        walk.visit(child, 1)?;
    }

    let mut demands = root_demands;
    let mut node_map: BTreeMap<i32, Material> = BTreeMap::new();
    let mut postorder_items = Vec::with_capacity(walk.order.len());
    for &(item, _) in &walk.order {
        postorder_items.push(item);
    }

    for (item, depth) in walk.order.into_iter().rev() {
        let needed = demands.get(&item).copied().unwrap_or(0);
        if needed == 0 {
            continue;
        }
        let owned_amt = owned.get(&item).copied().unwrap_or(0).clamp(0, needed);
        let mut line = Material {
            item,
            needed,
            owned: owned_amt,
            depth,
            ..Default::default()
        };
        if let Some(id) = choices.get(&item) {
            let recipe = recipes.get(id).ok_or("Recipe unavailable")?;
            if recipe.output != item || recipe.yield_amount <= 0 {
                return Err("Invalid recipe choice".into());
            }
            let remaining = needed - owned_amt;
            let crafts = if remaining > 0 {
                (remaining + recipe.yield_amount - 1) / recipe.yield_amount
            } else {
                0
            };
            line.recipe = Some(*id);
            line.crafts = crafts;
            line.surplus = crafts
                .checked_mul(recipe.yield_amount)
                .and_then(|total| total.checked_sub(remaining))
                .ok_or("Quantity too large")?;
            for (child, amount) in &recipe.ingredients {
                if *amount <= 0 || excluded.contains(child) {
                    continue;
                }
                if *child == item {
                    return Err(
                        "These craft choices form a cycle. Choose Buy for one of the ingredients."
                            .into(),
                    );
                }
                let added = crafts.checked_mul(*amount).ok_or("Quantity too large")?;
                if added > 0 {
                    let demand = demands.entry(*child).or_insert(0_i64);
                    *demand = demand
                        .checked_add(added)
                        .filter(|n| *n <= 1_000_000_000)
                        .ok_or("Quantity too large")?;
                    *consumers_map
                        .entry(*child)
                        .or_default()
                        .entry((*id, item))
                        .or_insert(0_i64) += added;
                }
            }
        }
        node_map.insert(item, line);
    }

    // Bottom-up topological order for crafted intermediates (dependencies first).
    let mut steps = Vec::new();
    for item in postorder_items {
        if let Some(line) = node_map.get(&item)
            && line.recipe.is_some()
        {
            steps.push(line.clone());
        }
    }

    // Deterministic `BTreeMap` iteration guarantees sorted buyable leaves.
    let materials: Vec<Material> = node_map
        .into_values()
        .filter(|line| line.recipe.is_none() && line.remaining() > 0)
        .collect();

    let consumers = consumers_map
        .into_iter()
        .map(|(item, parents)| {
            let list = parents
                .into_iter()
                .map(
                    |((target_recipe_id, target_item_id), units_used)| StepContribution {
                        target_recipe_id,
                        target_item_id,
                        units_used,
                    },
                )
                .collect();
            (item, list)
        })
        .collect();

    Ok(Expansion {
        materials,
        steps,
        targets: target_materials,
        consumers,
    })
}

/// Evaluate purchase readiness for a target or intermediate crafting step.
///
/// `acquired` maps buyable item ID -> units already bought/owned in Shop.
/// `crafted_units` maps intermediate item ID -> yield units completed (or `crafted * yield`).
/// `on_hand` maps intermediate item ID -> pre-owned intermediate stock before crafting.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_step_readiness(
    needed: i64,
    completed: i64,
    crafts: i64,
    yield_amount: i64,
    recipe: &Recipe,
    excluded: &BTreeSet<i32>,
    crafted_choices: &BTreeMap<i32, bool>,
    acquired: &BTreeMap<i32, i64>,
    crafted_units: &BTreeMap<i32, i64>,
    on_hand: &BTreeMap<i32, i64>,
) -> StepReadiness {
    if needed <= 0 || completed >= needed {
        return StepReadiness::Done;
    }
    if crafts <= 0 {
        return StepReadiness::Done;
    }
    let yield_amount = yield_amount.max(1);
    let remaining_crafts = ((needed - completed).max(0) + yield_amount - 1) / yield_amount;
    let mut craftable_now = remaining_crafts;
    let mut missing_kinds = 0_usize;
    for &(child, per_craft) in &recipe.ingredients {
        if per_craft <= 0 || excluded.contains(&child) {
            continue;
        }
        let available = if crafted_choices.get(&child).copied().unwrap_or(false) {
            on_hand.get(&child).copied().unwrap_or(0).max(0)
                + crafted_units.get(&child).copied().unwrap_or(0).max(0)
        } else {
            on_hand.get(&child).copied().unwrap_or(0).max(0)
                + acquired.get(&child).copied().unwrap_or(0).max(0)
        };
        let can_cover = available / per_craft;
        if can_cover == 0 {
            missing_kinds += 1;
        }
        craftable_now = craftable_now.min(can_cover);
    }
    if missing_kinds > 0 || craftable_now == 0 {
        StepReadiness::NeedMaterials { missing_kinds }
    } else {
        StepReadiness::Ready { craftable_now }
    }
}

/// Optimize `Craft` vs `Buy` decisions across all intermediate sub-crafts using
/// whole-stack market board and optional NPC vendor pricing.
#[allow(clippy::too_many_arguments)]
pub fn optimize_craft_choices(
    targets: &[TargetDemand],
    recipes: &BTreeMap<i32, Recipe>,
    default_recipes: &BTreeMap<i32, i32>,
    current_choices: &BTreeMap<i32, Option<i32>>,
    owned: &BTreeMap<i32, i64>,
    excluded: &BTreeSet<i32>,
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    limits: ExpandLimits,
) -> Result<OptimizePlan, String> {
    // Resolve every intermediate's preferred craft recipe ID (explicit choice
    // if `Some`, otherwise `default_recipes`). Root target outputs themselves are
    // not candidates unless used as an intermediate.
    let mut candidate_recipe: BTreeMap<i32, i32> = default_recipes.clone();
    for (&item_id, choice) in current_choices {
        if let Some(recipe_id) = choice {
            candidate_recipe.insert(item_id, *recipe_id);
        }
    }

    let baseline_craft_exp =
        expand_many(targets, recipes, &candidate_recipe, owned, excluded, limits)?;

    let mut purchase_cache: BTreeMap<(i32, i64), (i64, i64)> = BTreeMap::new();
    let mut eval_cost = |choices_map: &BTreeMap<i32, i32>| -> Result<(i64, i64), String> {
        let exp = expand_many(targets, recipes, choices_map, owned, excluded, limits)?;
        let mut total_missing = 0_i64;
        let mut total_cost = 0_i64;
        for m in &exp.materials {
            let need = m.remaining();
            if need <= 0 {
                continue;
            }
            let (missing, cost) = *purchase_cache.entry((m.item, need)).or_insert_with(|| {
                let offers = market.get(&m.item).map(Vec::as_slice).unwrap_or(&[]);
                let p = purchase(need, offers, vendors.get(&m.item).copied());
                (p.missing(), p.cost)
            });
            total_missing = total_missing.saturating_add(missing);
            total_cost = total_cost.saturating_add(cost);
        }
        Ok((total_missing, total_cost))
    };

    let (craft_all_missing, craft_all_cost) = eval_cost(&candidate_recipe)?;
    let (buy_all_missing, buy_all_cost) = eval_cost(&BTreeMap::new())?;

    let mut current_eval_choices = candidate_recipe.clone();
    for (&item_id, choice) in current_choices {
        if choice.is_none() {
            current_eval_choices.remove(&item_id);
        }
    }
    let (current_missing, current_cost) = eval_cost(&current_eval_choices)?;

    // Work bottom-up across the full-craft dependency list (deepest steps first).
    // When there are <= 64 intermediates (every normal FFXIV project), compare
    // exact project totals so batch yield rounding and shared-node interactions
    // are priced accurately; if a step has no remaining demand (covered by
    // on_hand or pruned by an earlier parent switch), keep Craft cleanly.
    let mut working = candidate_recipe.clone();
    for step in baseline_craft_exp.steps.iter().take(64) {
        let Some(&step_recipe) = candidate_recipe.get(&step.item) else {
            continue;
        };
        let mut with_craft = working.clone();
        with_craft.insert(step.item, step_recipe);
        let mut with_buy = working.clone();
        with_buy.remove(&step.item);

        let craft_score = eval_cost(&with_craft)?;
        let buy_score = eval_cost(&with_buy)?;

        // Prefer Craft on exact ties; pick Buy only when buying is strictly
        // better (fewer missing units, or equal missing units and strictly cheaper).
        if (buy_score.0, buy_score.1) < (craft_score.0, craft_score.1) {
            working = with_buy;
        } else {
            working = with_craft;
        }
    }

    let (total_missing, total_cost) = eval_cost(&working)?;
    let savings_vs_current = if total_missing <= current_missing && current_cost > total_cost {
        current_cost.saturating_sub(total_cost)
    } else {
        0
    };

    let mut result_choices = BTreeMap::new();
    let mut flipped_items = Vec::new();
    for step in &baseline_craft_exp.steps {
        let chosen = working.get(&step.item).copied();
        let previous = current_choices
            .get(&step.item)
            .copied()
            .unwrap_or_else(|| candidate_recipe.get(&step.item).copied());
        if chosen.is_some() != previous.is_some() {
            flipped_items.push(step.item);
        }
        result_choices.insert(step.item, chosen);
    }

    Ok(OptimizePlan {
        choices: result_choices,
        total_cost,
        total_missing,
        craft_all_cost,
        craft_all_missing,
        buy_all_cost,
        buy_all_missing,
        savings_vs_current,
        flipped_items,
    })
}

/// Compare consolidated material shopping cost against buying each target item finished.
pub fn project_cost_summary(
    targets: &[TargetDemand],
    recipes: &BTreeMap<i32, Recipe>,
    craft_shopping_cost: Option<i64>,
    craft_missing_units: i64,
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
) -> ProjectCostSummary {
    let mut target_count = 0_usize;
    let mut priced_targets = 0_usize;
    let mut buy_finished_cost = 0_i64;
    let mut buy_finished_missing = 0_i64;

    for target in targets {
        if target.count <= 0 {
            continue;
        }
        let Some(recipe) = recipes.get(&target.recipe_id) else {
            continue;
        };
        target_count += 1;
        let offers = market.get(&recipe.output).map(Vec::as_slice).unwrap_or(&[]);
        let p = purchase(target.count, offers, vendors.get(&recipe.output).copied());
        if p.missing() == 0 {
            priced_targets += 1;
        } else {
            buy_finished_missing = buy_finished_missing.saturating_add(p.missing());
        }
        buy_finished_cost = buy_finished_cost.saturating_add(p.cost);
    }

    let finished_opt = (target_count > 0 && priced_targets > 0).then_some(buy_finished_cost);
    let savings_vs_buying_finished = match (craft_shopping_cost, finished_opt) {
        (Some(craft), Some(finished)) if craft_missing_units == 0 && buy_finished_missing == 0 => {
            Some(finished.saturating_sub(craft))
        }
        _ => None,
    };

    ProjectCostSummary {
        craft_shopping_cost,
        craft_missing_units,
        buy_finished_cost: finished_opt,
        buy_finished_missing_units: buy_finished_missing,
        buy_finished_priced_targets: priced_targets,
        target_count,
        savings_vs_buying_finished,
    }
}

/// Parent-first order means every demand for a shared intermediate is known
/// before rounding its batch count. Reverse this order for crafting instructions.
pub fn expand(
    root: &Recipe,
    quantity: i64,
    recipes: &BTreeMap<i32, Recipe>,
    choices: &BTreeMap<i32, i32>,
    owned: &BTreeMap<i32, i64>,
    excluded: &BTreeSet<i32>,
) -> Result<Vec<Material>, String> {
    struct Walk<'a> {
        recipes: &'a BTreeMap<i32, Recipe>,
        selected: BTreeMap<i32, i32>,
        excluded: &'a BTreeSet<i32>,
        active: BTreeSet<i32>,
        seen: BTreeSet<i32>,
        order: Vec<(i32, usize)>,
    }
    impl Walk<'_> {
        fn visit(&mut self, item: i32, depth: usize) -> Result<(), String> {
            if self.active.contains(&item) {
                return Err(
                    "These craft choices form a cycle. Choose Buy for one of the ingredients."
                        .into(),
                );
            }
            if self.seen.contains(&item) {
                return Ok(());
            }
            if depth > 12 || self.seen.len() >= 128 {
                return Err(
                    "This crafting plan is too large. Buy some intermediates to simplify it."
                        .into(),
                );
            }
            self.active.insert(item);
            if let Some(id) = self.selected.get(&item) {
                let recipe = self.recipes.get(id).ok_or("Recipe unavailable")?;
                if recipe.output != item || recipe.yield_amount <= 0 {
                    return Err("Invalid recipe choice".into());
                }
                for (child, amount) in &recipe.ingredients {
                    if *amount > 0 && !self.excluded.contains(child) {
                        self.visit(*child, depth + 1)?;
                    }
                }
            }
            self.active.remove(&item);
            self.seen.insert(item);
            self.order.push((item, depth));
            Ok(())
        }
    }
    let mut walk = Walk {
        recipes,
        selected: choices.clone(),
        excluded,
        active: BTreeSet::new(),
        seen: BTreeSet::new(),
        order: Vec::new(),
    };
    walk.selected.insert(root.output, root.id);
    walk.visit(root.output, 0)?;
    let mut demands = BTreeMap::from([(root.output, quantity.clamp(1, 9999))]);
    let mut result = Vec::new();
    for (item, depth) in walk.order.into_iter().rev() {
        let needed = demands.get(&item).copied().unwrap_or(0);
        if needed == 0 {
            continue;
        }
        let owned = if item == root.output {
            0
        } else {
            owned.get(&item).copied().unwrap_or(0).clamp(0, needed)
        };
        let mut line = Material {
            item,
            needed,
            owned,
            depth,
            ..Default::default()
        };
        if let Some(id) = walk.selected.get(&item) {
            let recipe = &recipes[id];
            let crafts = (needed - owned + recipe.yield_amount - 1) / recipe.yield_amount;
            line.recipe = Some(*id);
            line.crafts = crafts;
            line.surplus = crafts * recipe.yield_amount - (needed - owned);
            for (child, amount) in &recipe.ingredients {
                if *amount <= 0 || excluded.contains(child) {
                    continue;
                }
                let added = crafts.checked_mul(*amount).ok_or("Quantity too large")?;
                let demand = demands.entry(*child).or_insert(0_i64);
                *demand = demand
                    .checked_add(added)
                    .filter(|n| *n <= 1_000_000_000)
                    .ok_or("Quantity too large")?;
            }
        }
        result.push(line);
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct Offer {
    pub id: i64,
    pub world: i32,
    pub quantity: i64,
    pub price: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct Purchase {
    pub needed: i64,
    pub quantity: i64,
    pub cost: i64,
    pub vendor_quantity: i64,
    pub offers: Vec<Offer>,
    pub approximate: bool,
}

impl Purchase {
    pub fn missing(&self) -> i64 {
        (self.needed - self.quantity).max(0)
    }
}

fn finish(needed: i64, offers: Vec<Offer>, vendor: Option<i64>, approximate: bool) -> Purchase {
    let quantity = offers.iter().map(|o| o.quantity).sum::<i64>();
    let vendor_quantity = if vendor.is_some() {
        (needed - quantity).max(0)
    } else {
        0
    };
    Purchase {
        needed,
        quantity: quantity + vendor_quantity,
        cost: offers.iter().map(|o| o.quantity * o.price).sum::<i64>()
            + vendor_quantity * vendor.unwrap_or(0),
        offers,
        vendor_quantity,
        approximate,
    }
}

/// Knapsack cells (`needed * offers`) `purchase` solves exactly. Route search
/// re-solves every item once per candidate world set, so it stays small.
pub const PURCHASE_BUDGET: usize = 200_000;

/// Bounded 0/1 knapsack: listings cannot be split or reused. Vendor supply can
/// fill any remaining quantity. Large batches use two greedy candidates and
/// explicitly mark the result as a best-found estimate, never an exact optimum.
pub fn purchase(needed: i64, offers: &[Offer], vendor: Option<i64>) -> Purchase {
    purchase_with_budget(needed, offers, vendor, PURCHASE_BUDGET)
}

/// [`purchase`] with a caller-chosen exact-solve budget in knapsack cells, for
/// callers that solve one item once rather than once per route.
pub fn purchase_with_budget(
    needed: i64,
    offers: &[Offer],
    vendor: Option<i64>,
    budget: usize,
) -> Purchase {
    let vendor = vendor.filter(|p| *p > 0);
    let mut seen = BTreeSet::new();
    let offers: Vec<_> = offers
        .iter()
        .filter(|o| o.price > 0 && o.quantity > 0 && seen.insert(o.id))
        .cloned()
        .collect();
    if needed <= 0 {
        return Purchase::default();
    }
    let supply: i64 = offers.iter().map(|o| o.quantity).sum();
    if supply < needed && vendor.is_none() {
        return finish(needed, offers, None, false);
    }
    if needed > 10_000 || needed as usize * offers.len() > budget {
        let mut candidates = Vec::new();
        for by_stack in [false, true] {
            let mut sorted = offers.clone();
            sorted.sort_by_key(|o| {
                (
                    if by_stack {
                        o.price * o.quantity
                    } else {
                        o.price
                    },
                    o.id,
                )
            });
            let mut selected = Vec::new();
            let mut remaining = needed;
            for offer in sorted {
                if remaining <= 0 {
                    break;
                }
                if vendor.is_some_and(|p| {
                    offer.quantity * offer.price >= p * remaining.min(offer.quantity)
                }) {
                    continue;
                }
                remaining -= offer.quantity;
                selected.push(offer);
            }
            candidates.push(finish(needed, selected, vendor, true));
        }
        return candidates
            .into_iter()
            .min_by_key(|p| (p.missing(), p.cost))
            .unwrap();
    }
    let target = needed as usize;
    // Arena-backed paths keep predecessors immutable during descending updates.
    let mut paths: Vec<(usize, Option<usize>)> = Vec::new();
    let mut states = vec![(i64::MAX, None); target + 1];
    states[0] = (0, None);
    for (index, offer) in offers.iter().enumerate() {
        for amount in (0..target).rev() {
            let (cost, path) = states[amount];
            if cost == i64::MAX {
                continue;
            }
            let next = (amount as i64 + offer.quantity).min(needed) as usize;
            let cost = cost + offer.price * offer.quantity;
            if cost < states[next].0 {
                paths.push((index, path));
                states[next] = (cost, Some(paths.len() - 1));
            }
        }
    }
    let best = (0..=target)
        .filter(|n| states[*n].0 != i64::MAX && (*n == target || vendor.is_some()))
        .min_by_key(|n| states[*n].0 + (needed - *n as i64) * vendor.unwrap_or(0))
        .unwrap_or(0);
    let mut path = states[best].1;
    let mut selected = Vec::new();
    while let Some(p) = path {
        let (index, prev) = paths[p];
        selected.push(offers[index].clone());
        path = prev;
    }
    selected.sort_by_key(|o| (o.world, o.id));
    finish(needed, selected, vendor, false)
}

/// Gil-equivalent price of travel. A world hop is one loading screen from the
/// aetheryte; a datacenter hop goes through the main menu and several loading
/// screens. Zero weights reproduce pure gil ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct TravelWeights {
    pub world_hop: i64,
    pub dc_hop: i64,
}

impl Default for TravelWeights {
    fn default() -> Self {
        Self {
            world_hop: 2_000,
            dc_hop: 10_000,
        }
    }
}

impl TravelWeights {
    /// Gil-equivalent distance of a travel shape: the card ordering axis and
    /// the travel part of `effective`.
    pub fn distance(&self, travel: Travel) -> i64 {
        (travel.dc_hops as i64)
            .saturating_mul(self.dc_hop)
            .saturating_add((travel.world_hops as i64).saturating_mul(self.world_hop))
    }
}

/// Hops beyond the worlds already on the itinerary. Entering a datacenter
/// lands on one of its worlds, so that first world is part of the DC hop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct Travel {
    pub dc_hops: usize,
    pub world_hops: usize,
}

/// Everything route scoring needs besides market data. No UI or network
/// dependencies, and fully value-comparable so it can live in a memo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct RouteContext {
    pub home: i32,
    /// World id to datacenter id. An unmapped world is treated as its own
    /// datacenter so it is charged a DC hop rather than assumed free.
    pub datacenters: BTreeMap<i32, i32>,
    pub weights: TravelWeights,
    /// Item to ticked-off listings, carried as snapshots: a bought listing
    /// leaves the market, and the price paid should not drift afterwards.
    pub locked: BTreeMap<i32, Vec<Offer>>,
    /// `(item, world)` pairs the user reported as not available.
    pub unavailable: BTreeSet<(i32, i32)>,
}

impl RouteContext {
    pub fn dc_of(&self, world: i32) -> i32 {
        self.datacenters.get(&world).copied().unwrap_or(-world)
    }

    /// Home plus every world holding a locked purchase: free to (re)visit.
    pub fn visited(&self) -> BTreeSet<i32> {
        let mut visited = BTreeSet::from([self.home]);
        visited.extend(self.locked.values().flatten().map(|o| o.world));
        visited
    }

    pub fn travel(&self, worlds: &BTreeSet<i32>) -> Travel {
        let visited = self.visited();
        let visited_dcs: BTreeSet<i32> = visited.iter().map(|w| self.dc_of(*w)).collect();
        let new_worlds: Vec<i32> = worlds.difference(&visited).copied().collect();
        let new_dcs: BTreeSet<i32> = new_worlds
            .iter()
            .map(|w| self.dc_of(*w))
            .filter(|dc| !visited_dcs.contains(dc))
            .collect();
        Travel {
            dc_hops: new_dcs.len(),
            world_hops: new_worlds.len() - new_dcs.len(),
        }
    }

    pub fn travel_cost(&self, travel: Travel) -> i64 {
        self.weights.distance(travel)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct ShoppingPlan {
    pub purchases: BTreeMap<i32, Purchase>,
    /// Every non-home world with a purchase, locked stops included.
    pub worlds: BTreeSet<i32>,
    pub cost: i64,
    pub missing: i64,
    pub approximate: bool,
    pub travel: Travel,
    /// Gil cost plus the gil-equivalent travel cost: the ranking metric.
    pub effective: i64,
}

/// The card's comparison line against the savings basis: the first
/// (shortest-trip) card that completes the shopping plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavingLine {
    /// This card cannot finish the shopping plan; its partial cost is not compared.
    Incomplete,
    /// The basis card itself; `alone` when it is the only card.
    Basis { alone: bool },
    /// Complete and cheaper than the basis by this much gil.
    Saved(i64),
    /// Complete and dearer than the basis by this much gil (a pinned shared
    /// route can cost more than the frontier).
    Dearer(i64),
    /// Complete and priced the same as the basis.
    Same,
}

/// Index of the first card that completes the shopping plan. Comparing against
/// the no-travel card is misleading when home is short: its partial cost
/// can be lower than a route that actually finishes the shopping plan.
pub fn savings_basis(plans: &[ShoppingPlan]) -> Option<usize> {
    plans.iter().position(|p| p.missing == 0)
}

/// Describe a route against an explicit complete baseline. Incomplete routes
/// report missing supply; an incomplete or absent baseline cannot claim savings.
pub fn saving_line(
    plans: &[ShoppingPlan],
    basis: Option<usize>,
    index: usize,
) -> Option<SavingLine> {
    let plan = plans.get(index)?;
    if plan.missing > 0 {
        return Some(SavingLine::Incomplete);
    }
    let basis = basis?;
    let basis_plan = plans.get(basis)?;
    if basis_plan.missing > 0 {
        return None;
    }
    if basis == index {
        return Some(SavingLine::Basis {
            alone: plans.len() == 1,
        });
    }
    Some(match basis_plan.cost - plan.cost {
        d if d > 0 => SavingLine::Saved(d),
        d if d < 0 => SavingLine::Dearer(-d),
        _ => SavingLine::Same,
    })
}

/// Compare adjacent route alternatives, not a claimed price for one added world.
/// Frontier world sets need not be nested and pinned routes can be dearer.
/// Never subtract a partial cost, including when a pinned incomplete route
/// interrupts the complete frontier.
pub fn marginal_saving_line(plans: &[ShoppingPlan], index: usize) -> Option<SavingLine> {
    let previous = index.checked_sub(1)?;
    if plans.get(previous)?.missing > 0 || plans.get(index)?.missing > 0 {
        return None;
    }
    saving_line(plans, Some(previous), index)
}

/// Stable display order independent of the solver branch (exact, greedy,
/// insufficient supply, or locked purchases). Listing IDs break price/stack
/// ties so checking a purchase never changes the ordering by itself.
pub fn itinerary(plan: &ShoppingPlan) -> BTreeMap<i32, Vec<(i32, Offer)>> {
    let mut stops: BTreeMap<i32, Vec<(i32, Offer)>> = BTreeMap::new();
    for (item, purchase) in &plan.purchases {
        for offer in &purchase.offers {
            stops
                .entry(offer.world)
                .or_default()
                .push((*item, offer.clone()));
        }
    }
    for rows in stops.values_mut() {
        rows.sort_by_key(|(item, o)| (*item, o.price, o.quantity, o.id));
    }
    stops
}

/// Locked listings are committed purchases: they count against the need at
/// the price they were ticked at, and only the remainder goes to the market.
fn purchase_with_locked(
    needed: i64,
    offers: &[Offer],
    locked: &[Offer],
    vendor: Option<i64>,
) -> Purchase {
    if locked.is_empty() {
        return purchase(needed, offers, vendor);
    }
    let committed: i64 = locked.iter().map(|o| o.quantity).sum();
    let inner = if needed > committed {
        purchase(needed - committed, offers, vendor)
    } else {
        Purchase::default()
    };
    let mut all = locked.to_vec();
    all.extend(inner.offers);
    all.sort_by_key(|o| (o.world, o.id));
    finish(needed, all, vendor.filter(|p| *p > 0), inner.approximate)
}

/// Per-item purchase results keyed by the exact listings the knapsack could
/// see, so route search does not re-solve an item whose visible offers did
/// not change between two world sets.
type PurchaseCache = BTreeMap<(i32, Vec<i64>), Purchase>;

fn shop_cached(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    allowed: &BTreeSet<i32>,
    ctx: &RouteContext,
    cache: &mut PurchaseCache,
) -> ShoppingPlan {
    let visited = ctx.visited();
    let mut plan = ShoppingPlan::default();
    for m in materials
        .iter()
        .filter(|m| m.recipe.is_none() && m.remaining() > 0)
    {
        let locked = ctx.locked.get(&m.item).map(Vec::as_slice).unwrap_or(&[]);
        let locked_ids: BTreeSet<i64> = locked.iter().map(|o| o.id).collect();
        let offers: Vec<_> = market
            .get(&m.item)
            .into_iter()
            .flatten()
            .filter(|o| allowed.contains(&o.world) || visited.contains(&o.world))
            .filter(|o| !ctx.unavailable.contains(&(m.item, o.world)))
            .filter(|o| !locked_ids.contains(&o.id))
            .cloned()
            .collect();
        let key = (m.item, offers.iter().map(|o| o.id).collect::<Vec<_>>());
        let p = cache
            .entry(key)
            .or_insert_with(|| {
                purchase_with_locked(
                    m.remaining(),
                    &offers,
                    locked,
                    vendors.get(&m.item).copied(),
                )
            })
            .clone();
        plan.worlds
            .extend(p.offers.iter().map(|o| o.world).filter(|w| *w != ctx.home));
        plan.cost += p.cost;
        plan.missing += p.missing();
        plan.approximate |= p.approximate;
        plan.purchases.insert(m.item, p);
    }
    plan.travel = ctx.travel(&plan.worlds);
    plan.effective = plan.cost.saturating_add(ctx.travel_cost(plan.travel));
    plan
}

/// Buy every outstanding leaf from `allowed` worlds (plus the worlds already
/// on the itinerary), honouring locked purchases and "not here" reports.
pub fn shop(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    allowed: &BTreeSet<i32>,
    ctx: &RouteContext,
) -> ShoppingPlan {
    shop_cached(
        materials,
        market,
        vendors,
        allowed,
        ctx,
        &mut PurchaseCache::new(),
    )
}

/// Completeness first, then the travel-weighted cost, then plain gil, then
/// the world set itself so ties resolve the same way on every run.
pub fn rank(p: &ShoppingPlan) -> (i64, i64, i64, BTreeSet<i32>) {
    (p.missing, p.effective, p.cost, p.worlds.clone())
}

pub const ROUTE_LIMIT: usize = 5;
const BEAM_WIDTH: usize = 5;
const BEAM_ROUNDS: usize = 4;

/// Best-found routes collapsed onto the travel frontier (see `frontier`).
/// Single-world additions are exhaustive; larger routes come from a bounded
/// beam plus whole-datacenter and full-scope seeds, so the UI calls these
/// best-found, not optimal. The no-travel plan is always evaluated and is
/// always the first card; the full-scope plan is always evaluated, so the
/// last card is always the most complete plan found and, among equally
/// complete plans, the cheapest — but a more complete plan can still cost
/// more gil than an earlier, incomplete one (see
/// `full_scope_can_reuse_a_better_home_plan_for_large_stacks`).
pub fn compare_routes(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    ctx: &RouteContext,
) -> RouteComparison {
    let visited = ctx.visited();
    let needed: BTreeSet<i32> = materials
        .iter()
        .filter(|m| m.recipe.is_none() && m.remaining() > 0)
        .map(|m| m.item)
        .collect();
    let candidates: BTreeSet<i32> = market
        .iter()
        .filter(|(item, _)| needed.contains(item))
        .flat_map(|(item, offers)| {
            offers
                .iter()
                .filter(move |o| !ctx.unavailable.contains(&(*item, o.world)))
                .map(|o| o.world)
        })
        .filter(|w| !visited.contains(w))
        .collect();
    let mut cache = PurchaseCache::new();
    let mut pool: BTreeMap<BTreeSet<i32>, ShoppingPlan> = BTreeMap::new();
    // Scoped so the closure's borrow of `pool` ends before the collapse below.
    {
        let mut evaluate = |allowed: BTreeSet<i32>| {
            let plan = pool.entry(allowed.clone()).or_insert_with(|| {
                shop_cached(materials, market, vendors, &allowed, ctx, &mut cache)
            });
            rank(plan)
        };
        let mut beam = vec![(evaluate(visited.clone()), visited.clone())];
        for _ in 0..BEAM_ROUNDS {
            let mut next = beam.clone();
            for (_, set) in &beam {
                for world in candidates.difference(set) {
                    let mut allowed = set.clone();
                    allowed.insert(*world);
                    if !next.iter().any(|(_, s)| *s == allowed) {
                        let key = evaluate(allowed.clone());
                        next.push((key, allowed));
                    }
                }
            }
            next.sort();
            next.truncate(BEAM_WIDTH);
            if next == beam {
                break;
            }
            beam = next;
        }
        let mut full = visited.clone();
        full.extend(&candidates);
        evaluate(full);
        let datacenters: BTreeSet<i32> = candidates.iter().map(|w| ctx.dc_of(*w)).collect();
        for dc in datacenters {
            let mut allowed = visited.clone();
            allowed.extend(candidates.iter().filter(|w| ctx.dc_of(**w) == dc));
            evaluate(allowed);
        }
    }
    RouteComparison {
        cards: frontier(pool.into_values(), &ctx.weights, ROUTE_LIMIT),
    }
}

/// The route cards: a Pareto frontier over (travel distance, gil), shortest
/// trip first. `cards[0]` is the no-new-travel baseline and the last card is
/// the most complete plan found and, among equally complete plans, the
/// cheapest — it is not necessarily cheaper in gil than an earlier,
/// incomplete card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct RouteComparison {
    pub cards: Vec<ShoppingPlan>,
}

impl RouteComparison {
    pub fn baseline(&self) -> Option<&ShoppingPlan> {
        self.cards.first()
    }

    pub fn cheapest(&self) -> Option<&ShoppingPlan> {
        self.cards.last()
    }

    /// Index of the card `rank` prefers given the travel weights: the
    /// default selection and the "Best value" badge.
    pub fn best_value(&self) -> Option<usize> {
        (0..self.cards.len()).min_by_key(|i| rank(&self.cards[*i]))
    }
}

/// Collapse candidate plans onto the travel frontier. One card per travel
/// shape (the plan with the fewest missing units, then the least gil), sorted
/// by `weights.distance` then hop counts, keeping only cards that strictly
/// improve on the card to their left (fewer missing, or equal missing and
/// less gil). The baseline shape (no new travel) has distance zero and sorts
/// first, so it is always kept. Longer frontiers are trimmed to `limit`
/// (at least two) keeping the first and last cards, the min-`rank` card (so
/// `best_value` never points at a card the trim discarded), and then the
/// steps with the largest marginal improvement over their left neighbour.
pub fn frontier(
    plans: impl IntoIterator<Item = ShoppingPlan>,
    weights: &TravelWeights,
    limit: usize,
) -> Vec<ShoppingPlan> {
    let mut by_shape: BTreeMap<Travel, ShoppingPlan> = BTreeMap::new();
    for plan in plans {
        let better = by_shape.get(&plan.travel).is_none_or(|cur| {
            (plan.missing, plan.cost, &plan.worlds) < (cur.missing, cur.cost, &cur.worlds)
        });
        if better {
            by_shape.insert(plan.travel, plan);
        }
    }
    let mut shapes: Vec<ShoppingPlan> = by_shape.into_values().collect();
    shapes.sort_by_key(|p| (weights.distance(p.travel), p.travel));
    let mut kept: Vec<ShoppingPlan> = Vec::new();
    for plan in shapes {
        let improves = kept
            .last()
            .is_none_or(|last| (plan.missing, plan.cost) < (last.missing, last.cost));
        if improves {
            kept.push(plan);
        }
    }
    let limit = limit.max(2);
    if kept.len() <= limit {
        return kept;
    }
    let last = kept.len() - 1;
    // The min-rank card is the default selection ("Best value"); it must
    // survive the trim even when its marginal saving over its left neighbour
    // is small, or `best_value()` (computed after trimming) could name a
    // card the engine already discarded as worse.
    let best = (0..kept.len()).min_by_key(|i| rank(&kept[*i])).unwrap_or(0);
    // Marginal improvement over the left neighbour: completing more of the
    // recipe outranks saving gil. Ties keep the shorter trip.
    let mut middle: Vec<(usize, (i64, i64))> = (1..last)
        .map(|i| {
            (
                i,
                (
                    kept[i - 1].missing - kept[i].missing,
                    kept[i - 1].cost - kept[i].cost,
                ),
            )
        })
        .collect();
    middle.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut keep: BTreeSet<usize> = BTreeSet::from([0, last, best]);
    for (i, _) in middle {
        if keep.len() >= limit {
            break;
        }
        keep.insert(i);
    }
    kept.into_iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(i))
        .map(|(_, p)| p)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recipe(id: i32, output: i32, yield_amount: i64, ingredients: &[(i32, i64)]) -> Recipe {
        Recipe {
            id,
            output,
            yield_amount,
            ingredients: ingredients.to_vec(),
        }
    }
    fn offer(id: i32, world: i32, quantity: i64, price: i64) -> Offer {
        Offer {
            id: id.into(),
            world,
            quantity,
            price,
        }
    }
    fn ctx(home: i32, datacenters: &[(i32, i32)]) -> RouteContext {
        RouteContext {
            home,
            datacenters: datacenters.iter().copied().collect(),
            ..Default::default()
        }
    }
    fn leaf(item: i32, needed: i64) -> Material {
        Material {
            item,
            needed,
            ..Default::default()
        }
    }

    #[test]
    fn itinerary_order_survives_locking_for_every_purchase_branch() {
        // Price order differs from ID order, as on the reported Fire Cluster
        // itinerary. The locked path sorts by ID; greedy and partial paths do
        // not. The presentation must remain stable through tick and untick.
        let offers = vec![offer(2, 1, 6000, 1), offer(1, 1, 6000, 2)];
        for needed in [12000, 13000, 2] {
            let market = BTreeMap::from([(42, offers.clone())]);
            let materials = [leaf(42, needed)];
            let allowed = BTreeSet::from([1]);
            let mut context = ctx(1, &[(1, 1)]);
            let before = shop(&materials, &market, &BTreeMap::new(), &allowed, &context);
            let bought = before.purchases[&42]
                .offers
                .iter()
                .find(|o| o.id == 2)
                .unwrap()
                .clone();
            context.locked.insert(42, vec![bought]);
            let after = shop(&materials, &market, &BTreeMap::new(), &allowed, &context);
            assert_eq!(before.cost, after.cost);
            assert_eq!(itinerary(&before), itinerary(&after), "needed={needed}");
            context.locked.clear();
            assert_eq!(
                before,
                shop(&materials, &market, &BTreeMap::new(), &allowed, &context)
            );
        }
    }
    fn plan(travel: (usize, usize), cost: i64, missing: i64, worlds: &[i32]) -> ShoppingPlan {
        ShoppingPlan {
            travel: Travel {
                dc_hops: travel.0,
                world_hops: travel.1,
            },
            cost,
            missing,
            effective: cost,
            worlds: worlds.iter().copied().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn frontier_keeps_the_cheaper_plan_per_shape() {
        let w = TravelWeights::default();
        let cards = frontier(
            [
                plan((0, 0), 100, 0, &[]),
                plan((0, 1), 90, 0, &[2]),
                plan((0, 1), 80, 0, &[3]),
            ],
            &w,
            5,
        );
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[1].worlds, BTreeSet::from([3]));
    }

    #[test]
    fn frontier_drops_further_and_dearer_but_keeps_further_and_cheaper() {
        let w = TravelWeights::default();
        let cards = frontier(
            [
                plan((0, 0), 100, 0, &[]),
                plan((0, 1), 80, 0, &[2]),
                plan((0, 2), 85, 0, &[2, 3]),
                plan((1, 0), 60, 0, &[9]),
            ],
            &w,
            5,
        );
        let costs: Vec<i64> = cards.iter().map(|p| p.cost).collect();
        assert_eq!(costs, vec![100, 80, 60]);
    }

    #[test]
    fn frontier_always_keeps_the_baseline() {
        let w = TravelWeights::default();
        // Nothing beats home.
        let cards = frontier(
            [plan((0, 0), 100, 0, &[]), plan((0, 1), 100, 0, &[2])],
            &w,
            5,
        );
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].travel, Travel::default());
        // Home is incomplete and dearer-looking plans that complete are kept.
        let cards = frontier(
            [plan((0, 0), 10, 3, &[]), plan((0, 1), 500, 0, &[2])],
            &w,
            5,
        );
        assert_eq!(cards.len(), 2);
        assert_eq!((cards[0].missing, cards[1].missing), (3, 0));
    }

    #[test]
    fn frontier_trims_to_the_limit_keeping_ends_and_largest_savings() {
        let w = TravelWeights::default();
        // Seven strict steps; marginal savings: 1, 50, 2, 40, 3, 30.
        let cards = frontier(
            [
                plan((0, 0), 1000, 0, &[]),
                plan((0, 1), 999, 0, &[2]),
                plan((0, 2), 949, 0, &[2, 3]),
                plan((0, 3), 947, 0, &[2, 3, 4]),
                plan((0, 4), 907, 0, &[2, 3, 4, 5]),
                plan((1, 0), 904, 0, &[9]),
                plan((1, 1), 874, 0, &[9, 10]),
            ],
            &w,
            5,
        );
        let costs: Vec<i64> = cards.iter().map(|p| p.cost).collect();
        assert_eq!(costs, vec![1000, 949, 907, 904, 874]);
    }

    #[test]
    fn frontier_trim_keeps_the_best_value_card() {
        let w = TravelWeights::default();
        // Same seven strictly-improving shapes as the trim test above, but
        // the third middle card (a small marginal saving the old rule would
        // drop) is given a distinctly low `effective` so `rank` prefers it.
        let mut best = plan((0, 3), 947, 0, &[2, 3, 4]);
        best.effective = 1;
        let cards = frontier(
            [
                plan((0, 0), 1000, 0, &[]),
                plan((0, 1), 999, 0, &[2]),
                plan((0, 2), 949, 0, &[2, 3]),
                best,
                plan((0, 4), 907, 0, &[2, 3, 4, 5]),
                plan((1, 0), 904, 0, &[9]),
                plan((1, 1), 874, 0, &[9, 10]),
            ],
            &w,
            5,
        );
        assert_eq!(cards.len(), 5);
        let costs: Vec<i64> = cards.iter().map(|p| p.cost).collect();
        assert_eq!(costs, vec![1000, 949, 947, 907, 874]);
        assert_eq!(cards.first().map(|p| p.cost), Some(1000));
        assert_eq!(cards.last().map(|p| p.cost), Some(874));
        let comparison = RouteComparison { cards };
        assert_eq!(
            comparison.best_value().map(|i| comparison.cards[i].cost),
            Some(947)
        );
    }

    #[test]
    fn frontier_orders_by_travel_weight_then_hops() {
        let three_hops = plan((0, 3), 80, 0, &[2, 3, 4]);
        let one_dc = plan((1, 0), 70, 0, &[9]);
        let home = plan((0, 0), 100, 0, &[]);
        // Default weights: 3 world hops (6,000) sort before 1 DC hop (10,000).
        let cards = frontier(
            [home.clone(), three_hops.clone(), one_dc.clone()],
            &TravelWeights::default(),
            5,
        );
        assert_eq!(
            cards.iter().map(|p| p.cost).collect::<Vec<_>>(),
            vec![100, 80, 70]
        );
        // A cheap DC hop sorts first, and then 3 world hops are dearer-and-further.
        let cheap_dc = TravelWeights {
            world_hop: 2_000,
            dc_hop: 1_000,
        };
        let cards = frontier([home, three_hops, one_dc], &cheap_dc, 5);
        assert_eq!(
            cards.iter().map(|p| p.cost).collect::<Vec<_>>(),
            vec![100, 70]
        );
        assert_eq!(
            cheap_dc.distance(Travel {
                dc_hops: 1,
                world_hops: 2
            }),
            5_000
        );
    }

    #[test]
    fn best_value_follows_rank_not_position() {
        let mut middle = plan((0, 1), 80, 0, &[2]);
        middle.effective = 82;
        let mut last = plan((1, 0), 70, 0, &[9]);
        last.effective = 170;
        let c = RouteComparison {
            cards: vec![plan((0, 0), 100, 0, &[]), middle, last],
        };
        assert_eq!(c.best_value(), Some(1));
        assert_eq!(c.baseline().map(|p| p.cost), Some(100));
        assert_eq!(c.cheapest().map(|p| p.cost), Some(70));
        assert_eq!(RouteComparison::default().best_value(), None);
    }

    #[test]
    fn shared_intermediates_round_once_and_owned_is_consumed_once() {
        let root = recipe(1, 10, 1, &[(20, 1), (30, 1)]);
        let recipes = BTreeMap::from([
            (1, root.clone()),
            (2, recipe(2, 20, 1, &[(40, 2)])),
            (3, recipe(3, 30, 1, &[(40, 2)])),
            (4, recipe(4, 40, 3, &[(50, 9)])),
        ]);
        let choices = BTreeMap::from([(20, 2), (30, 3), (40, 4)]);
        let lines = expand(
            &root,
            1,
            &recipes,
            &choices,
            &BTreeMap::from([(40, 1)]),
            &BTreeSet::new(),
        )
        .unwrap();
        let shared = lines.iter().find(|l| l.item == 40).unwrap();
        assert_eq!(
            (shared.needed, shared.owned, shared.crafts, shared.surplus),
            (4, 1, 1, 0)
        );
        assert_eq!(lines.iter().find(|l| l.item == 50).unwrap().needed, 9);
    }
    #[test]
    fn batch_cost_does_not_amortize_surplus_and_cycles_fail() {
        let root = recipe(1, 10, 1, &[(20, 2)]);
        let mut recipes = BTreeMap::from([(1, root.clone()), (2, recipe(2, 20, 3, &[(30, 9)]))]);
        let choices = BTreeMap::from([(20, 2)]);
        let lines = expand(
            &root,
            1,
            &recipes,
            &choices,
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(lines.iter().find(|l| l.item == 20).unwrap().surplus, 1);
        let plan = shop(
            &lines,
            &BTreeMap::from([(30, vec![offer(1, 1, 9, 10)])]),
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &ctx(1, &[]),
        );
        assert_eq!(plan.cost, 90);
        recipes.get_mut(&2).unwrap().ingredients = vec![(10, 1)];
        assert!(
            expand(
                &root,
                1,
                &recipes,
                &choices,
                &BTreeMap::new(),
                &BTreeSet::new()
            )
            .is_err()
        );
    }
    #[test]
    fn whole_stacks_choose_lower_spend_not_lower_unit_price() {
        let p = purchase(2, &[offer(1, 1, 99, 1), offer(2, 1, 2, 10)], None);
        assert_eq!((p.quantity, p.cost), (2, 20));
        assert_eq!(p.offers[0].id, 2);
        let p = purchase(5, &[offer(1, 1, 3, 2)], Some(10));
        assert_eq!((p.vendor_quantity, p.cost), (2, 26));
    }
    #[test]
    fn vendors_complete_home_plan_while_cheaper_market_remains_an_option() {
        let materials = [leaf(1, 5)];
        let market = BTreeMap::from([(1, vec![offer(1, 2, 5, 8)])]);
        let vendors = BTreeMap::from([(1, 10)]);
        let context = ctx(1, &[(1, 10), (2, 10)]);
        let comparison = compare_routes(&materials, &market, &vendors, &context);
        let home = comparison.baseline().unwrap();
        assert_eq!(
            (home.cost, home.missing, home.purchases[&1].vendor_quantity),
            (50, 0, 5)
        );
        assert!(home.worlds.is_empty());
        assert_eq!(home.travel, Travel::default());
        assert_eq!(
            comparison.best_value(),
            Some(0),
            "10 gil saved should not justify a hop"
        );
        let cheapest = comparison.cheapest().unwrap();
        assert_eq!(
            (cheapest.cost, cheapest.purchases[&1].vendor_quantity),
            (40, 0)
        );
        assert_eq!(cheapest.worlds, BTreeSet::from([2]));

        let market_only = compare_routes(&materials, &market, &BTreeMap::new(), &context);
        assert_eq!(market_only.baseline().unwrap().missing, 5);
        assert_eq!(
            market_only.cards[market_only.best_value().unwrap()].worlds,
            BTreeSet::from([2])
        );
    }

    #[test]
    fn vendor_only_plan_needs_no_market_supply_or_world_visits() {
        let comparison = compare_routes(
            &[leaf(1, 5)],
            &BTreeMap::new(),
            &BTreeMap::from([(1, 10)]),
            &ctx(1, &[(1, 10)]),
        );
        assert_eq!(comparison.cards.len(), 1);
        let plan = &comparison.cards[0];
        assert_eq!(
            (plan.cost, plan.missing, plan.purchases[&1].vendor_quantity),
            (50, 0, 5)
        );
        assert!(plan.worlds.is_empty());
        assert!(itinerary(plan).is_empty());
    }

    #[test]
    fn stacks_are_not_reused_and_missing_supply_is_explicit() {
        let p = purchase(5, &[offer(1, 1, 3, 2), offer(1, 1, 3, 2)], None);
        assert_eq!((p.quantity, p.missing(), p.cost), (3, 2, 6));
    }
    #[test]
    fn itinerary_counts_nested_materials_and_prefers_complete_plans() {
        let lines = vec![
            Material {
                item: 1,
                needed: 2,
                ..Default::default()
            },
            Material {
                item: 2,
                needed: 1,
                ..Default::default()
            },
        ];
        let market = BTreeMap::from([
            (1, vec![offer(1, 1, 2, 50), offer(2, 2, 2, 10)]),
            (2, vec![offer(3, 2, 1, 20)]),
        ]);
        let c = compare_routes(
            &lines,
            &market,
            &BTreeMap::new(),
            &ctx(1, &[(1, 1), (2, 1)]),
        );
        // The incomplete home plan is the baseline; the complete plan is the
        // best value even though home looks cheaper.
        assert_eq!(c.cards[0].missing, 1);
        let best = &c.cards[c.best_value().unwrap()];
        assert_eq!((best.cost, best.missing), (40, 0));
        assert_eq!(best.worlds, BTreeSet::from([2]));
        assert_eq!(
            best.travel,
            Travel {
                dc_hops: 0,
                world_hops: 1
            }
        );
    }
    #[test]
    fn exact_stack_solver_matches_exhaustive_subsets() {
        let offers = vec![
            offer(1, 1, 3, 7),
            offer(2, 2, 5, 4),
            offer(3, 1, 2, 8),
            offer(4, 2, 8, 3),
        ];
        for need in 1..=20 {
            for vendor in [None, Some(6)] {
                let expected = (0..1 << offers.len())
                    .map(|mask| {
                        finish(
                            need,
                            offers
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| mask & (1 << i) != 0)
                                .map(|(_, o)| o.clone())
                                .collect(),
                            vendor,
                            false,
                        )
                    })
                    .min_by_key(|p| (p.missing(), p.cost))
                    .unwrap();
                let actual = purchase(need, &offers, vendor);
                assert_eq!(
                    (actual.missing(), actual.cost),
                    (expected.missing(), expected.cost)
                );
            }
        }
    }

    #[test]
    fn stack_solver_matches_varied_exhaustive_markets() {
        // A fixed seed keeps failures reproducible. The oracle enumerates all
        // subsets independently, including partial supply and vendor top-ups.
        let mut seed = 358_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed >> 32
        };
        for case in 0..256 {
            let offers: Vec<_> = (0..8)
                .map(|id| {
                    offer(
                        id,
                        1 + (id % 3),
                        1 + (next() % 12) as i64,
                        1 + (next() % 30) as i64,
                    )
                })
                .collect();
            let needed = 1 + (next() % 100) as i64;
            for vendor in [None, Some(1 + (next() % 30) as i64)] {
                let expected = (0..1_u32 << offers.len())
                    .map(|mask| {
                        let mut quantity = 0;
                        let mut cost = 0;
                        for (i, o) in offers.iter().enumerate() {
                            if mask & (1 << i) != 0 {
                                quantity += o.quantity;
                                cost += o.quantity * o.price;
                            }
                        }
                        let missing = (needed - quantity).max(0);
                        match vendor {
                            Some(price) => (0, cost + missing * price),
                            None => (missing, cost),
                        }
                    })
                    .min()
                    .unwrap();
                let actual = purchase(needed, &offers, vendor);
                assert_eq!((actual.missing(), actual.cost), expected, "market {case}");
                assert_eq!(
                    actual
                        .offers
                        .iter()
                        .map(|o| o.id)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    actual.offers.len()
                );
            }
        }
    }

    #[test]
    fn full_scope_can_reuse_a_better_home_plan_for_large_stacks() {
        let materials = [Material {
            item: 1,
            needed: 10_001,
            ..Default::default()
        }];
        let market = BTreeMap::from([(1, vec![offer(1, 1, 10_001, 2), offer(2, 2, 10_000, 1)])]);
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &ctx(1, &[]));
        // The best value is never worse than the no-travel plan, even when the
        // large-batch greedy path gets worse with more offers to choose from.
        assert_eq!(c.best_value(), Some(0));
        let home = c.baseline().unwrap();
        assert_eq!(home.cost, 20_002);
        assert!(home.worlds.is_empty());
        assert!(home.approximate);
    }

    #[test]
    fn travel_counts_a_new_datacenter_as_one_hop_plus_extra_worlds() {
        let c = ctx(1, &[(1, 10), (2, 10), (3, 20), (4, 20)]);
        let travel = |dc_hops, world_hops| Travel {
            dc_hops,
            world_hops,
        };
        assert_eq!(c.travel(&BTreeSet::new()), Travel::default());
        assert_eq!(c.travel(&BTreeSet::from([2])), travel(0, 1));
        assert_eq!(c.travel(&BTreeSet::from([3, 4])), travel(1, 1));
        assert_eq!(c.travel(&BTreeSet::from([2, 3, 4])), travel(1, 2));
        assert_eq!(c.travel_cost(travel(1, 2)), 10_000 + 2 * 2_000);
        // A locked purchase on world 3 means that datacenter is already on the
        // itinerary: its sibling is a plain world hop.
        let mut locked = c.clone();
        locked.locked.insert(9, vec![offer(1, 3, 1, 1)]);
        assert_eq!(locked.travel(&BTreeSet::from([3, 4])), travel(0, 1));
    }

    #[test]
    fn unmapped_world_is_charged_as_its_own_datacenter() {
        let c = ctx(1, &[(1, 10)]);
        assert_eq!(
            c.travel(&BTreeSet::from([7])),
            Travel {
                dc_hops: 1,
                world_hops: 0
            }
        );
    }

    #[test]
    fn datacenter_weight_can_outrank_a_cheaper_foreign_listing() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(1, vec![offer(1, 2, 1, 1_000), offer(2, 3, 1, 500)])]);
        let mut c_ctx = ctx(1, &[(1, 10), (2, 10), (3, 20)]);
        c_ctx.weights = TravelWeights {
            world_hop: 100,
            dc_hop: 5_000,
        };
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        let best = &c.cards[c.best_value().unwrap()];
        assert_eq!(best.worlds, BTreeSet::from([2]));
        assert_eq!((best.cost, best.effective), (1_000, 1_100));
        // Both travel shapes are on the frontier regardless of weights.
        assert_eq!(c.cards.len(), 3);
        c_ctx.weights = TravelWeights {
            world_hop: 0,
            dc_hop: 0,
        };
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        assert_eq!(c.cards[c.best_value().unwrap()].worlds, BTreeSet::from([3]));
    }

    #[test]
    fn locked_offer_is_kept_counted_and_makes_its_world_free() {
        let materials = [leaf(1, 2), leaf(2, 1)];
        // Item 1's locked listing is gone from the market: it was bought.
        let market = BTreeMap::from([
            (1, vec![offer(11, 1, 1, 100)]),
            (2, vec![offer(21, 1, 1, 100), offer(22, 3, 1, 95)]),
        ]);
        let mut c_ctx = ctx(1, &[(1, 10), (3, 20)]);
        c_ctx.locked.insert(1, vec![offer(10, 3, 1, 50)]);
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        let best = &c.cards[c.best_value().unwrap()];
        let first = &best.purchases[&1];
        assert_eq!((first.quantity, first.cost, first.missing()), (2, 150, 0));
        assert!(first.offers.iter().any(|o| o.id == 10));
        // World 3 is already on the itinerary, so the 5 gil saving is free.
        assert_eq!(best.purchases[&2].offers[0].id, 22);
        assert_eq!(best.worlds, BTreeSet::from([3]));
        assert_eq!(best.travel, Travel::default());
        assert_eq!(best.effective, best.cost);
    }

    #[test]
    fn locked_quantity_covering_the_need_buys_nothing_more() {
        let materials = [leaf(1, 2)];
        let market = BTreeMap::from([(1, vec![offer(11, 1, 5, 1)])]);
        let mut c = ctx(1, &[(1, 10)]);
        c.locked.insert(1, vec![offer(10, 1, 3, 7)]);
        let plan = shop(
            &materials,
            &market,
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        let p = &plan.purchases[&1];
        assert_eq!(
            (p.quantity, p.cost, p.missing(), p.offers.len()),
            (3, 21, 0, 1)
        );
    }

    #[test]
    fn locked_listing_still_on_the_market_is_not_bought_twice() {
        let materials = [leaf(1, 4)];
        let market = BTreeMap::from([(1, vec![offer(10, 1, 2, 5), offer(11, 1, 2, 9)])]);
        let mut c = ctx(1, &[(1, 10)]);
        c.locked.insert(1, vec![offer(10, 1, 2, 5)]);
        let plan = shop(
            &materials,
            &market,
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        let ids: Vec<_> = plan.purchases[&1].offers.iter().map(|o| o.id).collect();
        assert_eq!(ids, vec![10, 11]);
        assert_eq!((plan.cost, plan.missing), (28, 0));
    }

    #[test]
    fn locked_stock_reduces_the_vendor_top_up() {
        let materials = [leaf(1, 5)];
        let mut c = ctx(1, &[(1, 10)]);
        c.locked.insert(1, vec![offer(10, 1, 2, 3)]);
        let plan = shop(
            &materials,
            &BTreeMap::new(),
            &BTreeMap::from([(1, 10)]),
            &BTreeSet::from([1]),
            &c,
        );
        let p = &plan.purchases[&1];
        assert_eq!(
            (p.vendor_quantity, p.quantity, p.cost, p.missing()),
            (3, 5, 36, 0)
        );
    }

    #[test]
    fn unavailable_pair_is_skipped_without_blocking_the_world() {
        let materials = [leaf(1, 1), leaf(2, 1)];
        let market = BTreeMap::from([
            (1, vec![offer(11, 2, 1, 10), offer(12, 1, 1, 100)]),
            (2, vec![offer(21, 2, 1, 10)]),
        ]);
        let mut c_ctx = ctx(1, &[(1, 10), (2, 10)]);
        c_ctx.unavailable.insert((1, 2));
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        assert!(
            c.cards
                .iter()
                .flat_map(|p| p.purchases.get(&1))
                .flat_map(|p| &p.offers)
                .all(|o| o.world != 2)
        );
        let best = &c.cards[c.best_value().unwrap()];
        assert_eq!(best.purchases[&1].offers[0].id, 12);
        assert_eq!(best.purchases[&2].offers[0].id, 21);
        assert_eq!(best.missing, 0);
    }

    #[test]
    fn unavailable_does_not_drop_a_locked_offer() {
        let materials = [leaf(1, 1)];
        let mut c = ctx(1, &[(1, 10), (2, 10)]);
        c.locked.insert(1, vec![offer(10, 2, 1, 5)]);
        c.unavailable.insert((1, 2));
        let plan = shop(
            &materials,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        assert_eq!((plan.cost, plan.missing), (5, 0));
    }

    #[test]
    fn same_shape_routes_collapse_and_home_stays_best_value_for_small_savings() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(
            1,
            (1..=8)
                .map(|w| offer(w, w, 1, 100 - i64::from(w)))
                .collect::<Vec<_>>(),
        )]);
        let c_ctx = ctx(1, &(1..=8).map(|w| (w, 10)).collect::<Vec<_>>());
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &c_ctx);
        // Seven one-hop options collapse to the cheapest; two hops never help
        // a one-unit purchase, so the frontier is home and one hop.
        assert_eq!(c.cards.len(), 2);
        assert!(c.cards[0].worlds.is_empty());
        assert_eq!(c.cards[0].cost, 99);
        assert_eq!(c.cards[1].worlds, BTreeSet::from([8]));
        assert_eq!(c.cheapest().map(|p| p.cost), Some(92));
        // A few gil never pays for a 2,000 gil hop.
        assert_eq!(c.best_value(), Some(0));
    }

    #[test]
    fn a_single_world_scope_yields_one_route() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(1, vec![offer(1, 1, 1, 10)])]);
        let c = compare_routes(&materials, &market, &BTreeMap::new(), &ctx(1, &[(1, 10)]));
        assert_eq!(c.cards.len(), 1);
        assert_eq!(c.cards[0].cost, 10);
    }

    #[test]
    fn ranking_is_deterministic_under_ties_and_input_order() {
        let materials = [leaf(1, 1)];
        let forward = vec![offer(1, 3, 1, 10), offer(2, 2, 1, 10), offer(3, 1, 1, 50)];
        let mut backward = forward.clone();
        backward.reverse();
        let mut c = ctx(1, &[(1, 10), (2, 10), (3, 10)]);
        c.weights = TravelWeights {
            world_hop: 0,
            dc_hop: 0,
        };
        let a = compare_routes(
            &materials,
            &BTreeMap::from([(1, forward)]),
            &BTreeMap::new(),
            &c,
        );
        let b = compare_routes(
            &materials,
            &BTreeMap::from([(1, backward)]),
            &BTreeMap::new(),
            &c,
        );
        assert_eq!(a, b);
        // Equal gil and travel: the lower world set wins.
        assert_eq!(a.cards[a.best_value().unwrap()].worlds, BTreeSet::from([2]));
    }

    #[test]
    fn desired_output_rounds_up_and_nested_crystals_can_be_excluded() {
        let root = recipe(1, 10, 3, &[(20, 2), (59, 1)]);
        let recipes = BTreeMap::from([
            (1, root.clone()),
            (2, recipe(2, 20, 1, &[(30, 2), (59, 3)])),
        ]);
        let lines = expand(
            &root,
            4,
            &recipes,
            &BTreeMap::from([(20, 2)]),
            &BTreeMap::new(),
            &BTreeSet::from([59]),
        )
        .unwrap();
        assert_eq!((lines[0].crafts, lines[0].surplus), (2, 2));
        assert_eq!(lines.iter().find(|m| m.item == 30).unwrap().needed, 8);
        assert!(!lines.iter().any(|m| m.item == 59));
    }

    #[test]
    fn owned_intermediate_requires_no_child_purchases() {
        let root = recipe(1, 10, 1, &[(20, 2)]);
        let recipes = BTreeMap::from([(1, root.clone()), (2, recipe(2, 20, 3, &[(30, 9)]))]);
        let lines = expand(
            &root,
            1,
            &recipes,
            &BTreeMap::from([(20, 2)]),
            &BTreeMap::from([(20, 2)]),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(lines.iter().find(|m| m.item == 20).unwrap().crafts, 0);
        assert!(!lines.iter().any(|m| m.item == 30));
        let plan = shop(
            &lines,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &ctx(1, &[]),
        );
        assert_eq!((plan.cost, plan.missing), (0, 0));
    }
    #[test]
    fn saving_line_frames_each_card_against_the_first_complete_card() {
        let plan = |cost: i64, missing: i64| ShoppingPlan {
            cost,
            missing,
            ..Default::default()
        };
        // Home completes the recipe, so it is the basis. Later cards are
        // cheaper on the frontier, but a pinned shared route can be dearer
        // or priced the same, and each still says how it compares.
        let plans = vec![plan(1_000, 0), plan(800, 0), plan(1_200, 0), plan(1_000, 0)];
        assert_eq!(savings_basis(&plans), Some(0));
        assert_eq!(
            saving_line(&plans, Some(0), 0),
            Some(SavingLine::Basis { alone: false })
        );
        assert_eq!(
            saving_line(&plans, Some(0), 1),
            Some(SavingLine::Saved(200))
        );
        assert_eq!(
            saving_line(&plans, Some(0), 2),
            Some(SavingLine::Dearer(200))
        );
        assert_eq!(saving_line(&plans, Some(0), 3), Some(SavingLine::Same));
        let alone = vec![plan(1_000, 0)];
        assert_eq!(
            saving_line(&alone, Some(0), 0),
            Some(SavingLine::Basis { alone: true })
        );
        // Home is short: its partial cost is cheaper than any route that
        // finishes the craft, so it is marked incomplete rather than
        // compared, and the first complete card becomes the basis.
        let plans = vec![plan(10, 4), plan(900, 2), plan(1_000, 0), plan(700, 0)];
        assert_eq!(savings_basis(&plans), Some(2));
        assert_eq!(
            saving_line(&plans, Some(2), 0),
            Some(SavingLine::Incomplete)
        );
        assert_eq!(
            saving_line(&plans, Some(2), 1),
            Some(SavingLine::Incomplete)
        );
        assert_eq!(
            saving_line(&plans, Some(2), 2),
            Some(SavingLine::Basis { alone: false })
        );
        assert_eq!(
            saving_line(&plans, Some(2), 3),
            Some(SavingLine::Saved(300))
        );
        // Nothing completes: no basis, and every card is marked incomplete.
        let short = vec![plan(10, 4)];
        assert_eq!(savings_basis(&short), None);
        assert_eq!(saving_line(&short, None, 0), Some(SavingLine::Incomplete));
    }

    #[test]
    fn marginal_savings_compare_adjacent_complete_alternatives() {
        let plan = |cost, missing, worlds: &[i32]| ShoppingPlan {
            cost,
            missing,
            worlds: worlds.iter().copied().collect(),
            ..Default::default()
        };
        // The third route replaces world 79 instead of simply adding a hop.
        let plans = vec![
            plan(1_000, 0, &[]),
            plan(800, 0, &[79]),
            plan(700, 0, &[80, 81]),
        ];
        assert_eq!(marginal_saving_line(&plans, 0), None);
        assert_eq!(
            marginal_saving_line(&plans, 1),
            Some(SavingLine::Saved(200))
        );
        assert_eq!(
            marginal_saving_line(&plans, 2),
            Some(SavingLine::Saved(100))
        );
        assert_eq!(
            saving_line(&plans, savings_basis(&plans), 2),
            Some(SavingLine::Saved(300))
        );
        assert_eq!(marginal_saving_line(&plans, 3), None);
        // A pinned shared route can be dearer, equal, or incomplete. Do not
        // skip it when claiming an adjacent comparison or compare partial costs.
        let plans = vec![
            plan(10, 4, &[]),
            plan(800, 0, &[79]),
            plan(900, 0, &[80]),
            plan(900, 0, &[81]),
            plan(20, 2, &[82]),
            plan(700, 0, &[83]),
        ];
        assert_eq!(marginal_saving_line(&plans, 1), None);
        assert_eq!(
            marginal_saving_line(&plans, 2),
            Some(SavingLine::Dearer(100))
        );
        assert_eq!(marginal_saving_line(&plans, 3), Some(SavingLine::Same));
        assert_eq!(marginal_saving_line(&plans, 4), None);
        assert_eq!(marginal_saving_line(&plans, 5), None);
        assert_eq!(
            saving_line(&plans, savings_basis(&plans), 5),
            Some(SavingLine::Saved(100))
        );
        assert_eq!(marginal_saving_line(&[], 0), None);
    }

    #[test]
    fn savings_reject_partial_bases_even_with_equal_missing_units() {
        let plan = |cost, missing| ShoppingPlan {
            cost,
            missing,
            ..Default::default()
        };
        let plans = [plan(200, 1), plan(100, 1), plan(300, 0)];
        assert_eq!(marginal_saving_line(&plans, 1), None);
        assert_eq!(saving_line(&plans, Some(0), 2), None);
        assert_eq!(saving_line(&plans, Some(99), 2), None);
        assert_eq!(savings_basis(&plans), Some(2));
        assert_eq!(
            saving_line(&plans, Some(2), 2),
            Some(SavingLine::Basis { alone: false })
        );
    }

    #[test]
    fn expand_many_consolidates_shared_intermediates_before_yield_rounding() {
        // Both Sword (recipe 1, item 100) and Shield (recipe 2, item 101)
        // need 1x Iron Ingot (item 200, yield 2 per craft, needs 3x Ore 300).
        // Combined intermediate demand = 2 ingots => ONLY 1 ingot craft (3 ores),
        // NOT 2 separate rounds totaling 6 ores!
        let recipes = BTreeMap::from([
            (1, recipe(1, 100, 1, &[(200, 1), (400, 1)])),
            (2, recipe(2, 101, 1, &[(200, 1), (401, 2)])),
            (3, recipe(3, 200, 2, &[(300, 3)])),
        ]);
        let choices = BTreeMap::from([(200, 3)]);
        let exp = expand_many(
            &[
                TargetDemand {
                    recipe_id: 1,
                    count: 1,
                },
                TargetDemand {
                    recipe_id: 2,
                    count: 1,
                },
            ],
            &recipes,
            &choices,
            &BTreeMap::new(),
            &BTreeSet::new(),
            ExpandLimits::default(),
        )
        .unwrap();

        assert_eq!(exp.targets.len(), 2);
        assert_eq!(exp.steps.len(), 1);
        assert_eq!(exp.steps[0].item, 200);
        assert_eq!(exp.steps[0].needed, 2);
        assert_eq!(exp.steps[0].crafts, 1);
        assert_eq!(exp.steps[0].surplus, 0);

        let mat = |id| exp.materials.iter().find(|m| m.item == id).unwrap().needed;
        assert_eq!(mat(300), 3, "shared ingot batched to 1 craft of 3 ores");
        assert_eq!(mat(400), 1);
        assert_eq!(mat(401), 2);
        assert_eq!(exp.consumers[&200].len(), 2);
    }

    #[test]
    fn expand_many_orders_steps_bottom_up_deducts_on_hand_and_prunes_buy() {
        // Target 10 -> Step A (20, yield 1 -> 2x Step B) -> Step B (30, yield 1 -> 4x Ore 40)
        // On hand: 1x Step B -> only 1 craft of Step B needed -> 4x Ore.
        let recipes = BTreeMap::from([
            (1, recipe(1, 10, 1, &[(20, 1), (58, 2)])),
            (2, recipe(2, 20, 1, &[(30, 2)])),
            (3, recipe(3, 30, 1, &[(40, 4)])),
        ]);
        let choices = BTreeMap::from([(20, 2), (30, 3)]);
        let exp = expand_many(
            &[TargetDemand {
                recipe_id: 1,
                count: 1,
            }],
            &recipes,
            &choices,
            &BTreeMap::from([(30, 1)]),
            &BTreeSet::from([58]),
            ExpandLimits::default(),
        )
        .unwrap();

        assert_eq!(
            exp.steps.iter().map(|s| s.item).collect::<Vec<_>>(),
            vec![30, 20],
            "deepest dependency (30) must appear before parent step (20)"
        );
        assert_eq!(exp.steps[0].owned, 1);
        assert_eq!(exp.steps[0].crafts, 1);
        assert_eq!(exp.materials.len(), 1);
        assert_eq!((exp.materials[0].item, exp.materials[0].needed), (40, 4));

        // Flip 20 to Buy (remove from choices): Step 30 & Ore 40 disappear, item 20 becomes a leaf.
        let exp_buy = expand_many(
            &[TargetDemand {
                recipe_id: 1,
                count: 1,
            }],
            &recipes,
            &BTreeMap::new(),
            &BTreeMap::from([(30, 1)]),
            &BTreeSet::from([58]),
            ExpandLimits::default(),
        )
        .unwrap();
        assert!(exp_buy.steps.is_empty());
        assert_eq!(exp_buy.materials.len(), 1);
        assert_eq!(
            (exp_buy.materials[0].item, exp_buy.materials[0].needed),
            (20, 1)
        );
    }

    #[test]
    fn expand_many_detects_cycles_and_enforces_limits() {
        let cyclic = BTreeMap::from([
            (1, recipe(1, 10, 1, &[(20, 1)])),
            (2, recipe(2, 20, 1, &[(10, 1)])),
        ]);
        let err = expand_many(
            &[TargetDemand {
                recipe_id: 1,
                count: 1,
            }],
            &cyclic,
            &BTreeMap::from([(10, 1), (20, 2)]),
            &BTreeMap::new(),
            &BTreeSet::new(),
            ExpandLimits::default(),
        )
        .unwrap_err();
        assert!(err.contains("cycle"), "{err}");

        let wide_err = expand_many(
            &[
                TargetDemand {
                    recipe_id: 1,
                    count: 1,
                },
                TargetDemand {
                    recipe_id: 2,
                    count: 1,
                },
            ],
            &cyclic,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
            ExpandLimits::SINGLE_RECIPE,
        )
        .unwrap_err();
        assert!(wide_err.contains("too many targets"), "{wide_err}");
    }

    #[test]
    fn optimize_craft_choices_flips_cheaper_buy_keeps_craft_and_respects_missing_supply() {
        // Target 10 needs 2x CheapSub (20) + 1x DearSub (30).
        // CheapSub (20): mats cost 100 gil, MB buy costs 500 -> keep Craft!
        // DearSub (30): mats cost 1000 gil, MB buy costs 200 -> flip to Buy!
        // MissingSub (40): mats cost 50 gil, MB buy has NO offers -> keep Craft (never treat missing as 0 gil)!
        let recipes = BTreeMap::from([
            (1, recipe(1, 10, 1, &[(20, 2), (30, 1), (40, 1)])),
            (2, recipe(2, 20, 1, &[(21, 2)])),
            (3, recipe(3, 30, 1, &[(31, 5)])),
            (4, recipe(4, 40, 1, &[(41, 1)])),
        ]);
        let defaults = BTreeMap::from([(20, 2), (30, 3), (40, 4)]);
        // Whole-stack market listings (price per unit * stack quantity):
        let market = BTreeMap::from([
            (20, vec![offer(1, 1, 2, 500)]), // stack of 2 = 1000 gil
            (21, vec![offer(2, 1, 4, 50)]),  // stack of 4 = 200 gil (< 1000 -> keep Craft)
            (30, vec![offer(3, 1, 1, 200)]), // stack of 1 = 200 gil (< 1000 -> flip to Buy)
            (31, vec![offer(4, 1, 5, 200)]), // stack of 5 = 1000 gil
            (41, vec![offer(5, 1, 1, 50)]),  // stack of 1 = 50 gil
                                             // note item 40 has NO offers!
        ]);
        let plan = optimize_craft_choices(
            &[TargetDemand {
                recipe_id: 1,
                count: 1,
            }],
            &recipes,
            &defaults,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
            &market,
            &BTreeMap::new(),
            ExpandLimits::default(),
        )
        .unwrap();

        assert_eq!(plan.choices[&20], Some(2));
        assert_eq!(plan.choices[&30], None);
        assert_eq!(plan.choices[&40], Some(4));
        assert_eq!(plan.total_missing, 0);
        // 200 (item 21) + 200 (item 30 bought) + 50 (item 41) = 450
        assert_eq!(plan.total_cost, 450);
        assert_eq!(plan.craft_all_cost, 1250);
        assert_eq!(plan.savings_vs_current, 800);
        assert_eq!(plan.flipped_items, vec![30]);
    }

    #[test]
    fn step_readiness_tracks_acquired_and_child_crafts() {
        let ingot = recipe(2, 20, 1, &[(30, 2)]);
        let missing = evaluate_step_readiness(
            2,
            0,
            2,
            1,
            &ingot,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeMap::from([(30, 1)]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(missing, StepReadiness::NeedMaterials { missing_kinds: 1 });

        let one_ready = evaluate_step_readiness(
            2,
            0,
            2,
            1,
            &ingot,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeMap::from([(30, 3)]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(one_ready, StepReadiness::Ready { craftable_now: 1 });

        let done = evaluate_step_readiness(
            2,
            2,
            2,
            1,
            &ingot,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(done, StepReadiness::Done);
    }
}
