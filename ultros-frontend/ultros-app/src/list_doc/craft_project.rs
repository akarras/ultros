//! Shared client-side crafting project planner state used by both account
//! and device list workspaces (`Build | Craft | Shop`).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::OnceLock;

use ultros_api_types::ActiveListing;
use ultros_api_types::list::ListItem;
use ultros_calc::recipe_planner::{
    ExpandLimits, Material, Offer, OptimizePlan, Recipe, StepReadiness, TargetDemand,
    evaluate_step_readiness, expand_many, optimize_craft_choices, project_cost_summary, purchase,
};
use ultros_list_doc::{
    CraftChoiceSnapshot, CraftMode, CraftOptionsSnapshot, Quality, RowKey, RowSnapshot,
    TargetSnapshot,
};
use xiv_gen::{ItemId, RecipeId, RecipeLevelTableId};

use super::adapter::{row_id, to_list_item};
use crate::components::crafting_cost::{
    CRYSTAL_SEARCH_CATEGORY, IngredientsIter, vendor_price_map,
};
use crate::global_state::xiv_data::tracked_data;

const CRAFT_JOB_ABBRS: [&str; 8] = ["CRP", "BSM", "ARM", "GSM", "LTW", "WVR", "ALC", "CUL"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CraftTargetView {
    pub recipe_id: i32,
    pub item_id: i32,
    pub item_name: String,
    pub craft_type_abbr: &'static str,
    pub recipe_level: i32,
    pub count: i64,
    pub completed: i64,
    pub crafts: i64,
    pub yield_amount: i64,
    pub surplus: i64,
    pub completed_crafts: i64,
    pub readiness: StepReadiness,
    pub buy_finished_cost: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepIngredientView {
    pub item_id: i32,
    pub name: String,
    pub per_craft: i64,
    pub needed_total: i64,
    pub available: i64,
    pub is_crystal: bool,
    pub is_crafted_step: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CraftConsumerView {
    pub recipe_id: i32,
    pub item_id: i32,
    pub name: String,
    pub units_used: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CraftStepView {
    pub step_number: usize,
    pub item_id: i32,
    pub item_name: String,
    pub recipe_id: i32,
    pub craft_type_abbr: &'static str,
    pub recipe_level: i32,
    pub needed: i64,
    pub on_hand: i64,
    pub remaining: i64,
    pub crafts: i64,
    pub yield_amount: i64,
    pub surplus: i64,
    pub crafted: i64,
    pub mode: CraftMode,
    pub readiness: StepReadiness,
    pub consumers: Vec<CraftConsumerView>,
    pub craft_cost: Option<i64>,
    pub buy_cost: Option<i64>,
    pub ingredients: Vec<StepIngredientView>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CraftEstimateSummary {
    pub materials_cost: Option<i64>,
    pub buy_finished_cost: Option<i64>,
    pub savings: Option<i64>,
    pub unpriced_leaves: usize,
    pub steps_done: usize,
    pub steps_total: usize,
    pub targets_done: usize,
    pub targets_total: usize,
    pub leaves_acquired: i64,
    pub leaves_needed: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CraftProjectState {
    pub has_targets: bool,
    pub error: Option<String>,
    pub options: CraftOptionsSnapshot,
    pub targets: Vec<CraftTargetView>,
    pub steps: Vec<CraftStepView>,
    /// Explicit manual rows (`row.need > 0`) shown in Build mode.
    pub manual_rows: Vec<(ListItem, Vec<ActiveListing>)>,
    /// Merged manual rows + consolidated craft leaves shown in Shop and CartEstimate.
    pub consolidated_rows: Vec<(ListItem, Vec<ActiveListing>)>,
    /// Union of manual items, craft leaves, intermediate outputs, and target outputs.
    pub all_price_item_ids: BTreeSet<i32>,
    pub summary: CraftEstimateSummary,
}

/// Abstract recipe & item catalog so both live `xiv-gen-db` data and synthetic
/// unit test fixtures share the exact same consolidation and readiness code.
pub trait CraftDataSource {
    fn recipe(&self, recipe_id: i32) -> Option<Recipe>;
    fn default_recipe_for_item(&self, item_id: i32) -> Option<Recipe>;
    fn item_name(&self, item_id: i32) -> String;
    fn is_crystal(&self, item_id: i32) -> bool;
    fn recipe_job_and_level(&self, recipe_id: i32) -> (&'static str, i32);
    fn vendor_prices(&self, enabled: bool) -> BTreeMap<i32, i64>;
}

pub struct XivCraftDataSource;

impl XivCraftDataSource {
    fn convert_recipe(recipe: &xiv_gen::Recipe) -> Recipe {
        Recipe {
            id: recipe.key_id.0,
            output: recipe.item_result,
            yield_amount: i64::from(recipe.amount_result.max(1)),
            ingredients: IngredientsIter::new(recipe)
                .filter(|(id, amt)| id.0 > 0 && *amt > 0)
                .map(|(id, amt)| (id.0, i64::from(amt)))
                .collect(),
        }
    }

    fn default_recipe_ids() -> &'static HashMap<i32, i32> {
        static DEFAULTS: OnceLock<HashMap<i32, i32>> = OnceLock::new();
        DEFAULTS.get_or_init(|| {
            let mut map = HashMap::<i32, i32>::new();
            for (&recipe_id, recipe) in &tracked_data().recipes {
                if recipe.item_result <= 0 {
                    continue;
                }
                map.entry(recipe.item_result)
                    .and_modify(|existing| {
                        if recipe_id.0 < *existing {
                            *existing = recipe_id.0;
                        }
                    })
                    .or_insert(recipe_id.0);
            }
            map
        })
    }
}

impl CraftDataSource for XivCraftDataSource {
    fn recipe(&self, recipe_id: i32) -> Option<Recipe> {
        tracked_data()
            .recipes
            .get(&RecipeId(recipe_id))
            .map(Self::convert_recipe)
    }

    fn default_recipe_for_item(&self, item_id: i32) -> Option<Recipe> {
        let id = Self::default_recipe_ids().get(&item_id).copied()?;
        self.recipe(id)
    }

    fn item_name(&self, item_id: i32) -> String {
        tracked_data()
            .items
            .get(&ItemId(item_id))
            .map(|item| item.name.to_string())
            .unwrap_or_else(|| format!("Item #{item_id}"))
    }

    fn is_crystal(&self, item_id: i32) -> bool {
        tracked_data()
            .items
            .get(&ItemId(item_id))
            .is_some_and(|item| item.item_search_category == CRYSTAL_SEARCH_CATEGORY)
    }

    fn recipe_job_and_level(&self, recipe_id: i32) -> (&'static str, i32) {
        let data = tracked_data();
        let Some(recipe) = data.recipes.get(&RecipeId(recipe_id)) else {
            return ("", 0);
        };
        let abbr = CRAFT_JOB_ABBRS
            .get(recipe.craft_type as usize)
            .copied()
            .unwrap_or("");
        let level = data
            .recipe_level_tables
            .get(&RecipeLevelTableId(recipe.recipe_level_table))
            .map(|lvl| lvl.class_job_level)
            .unwrap_or(0);
        (abbr, i32::from(level))
    }

    fn vendor_prices(&self, enabled: bool) -> BTreeMap<i32, i64> {
        if !enabled {
            return BTreeMap::new();
        }
        vendor_price_map()
            .iter()
            .map(|(id, price)| (*id, i64::from(*price)))
            .collect()
    }
}

struct GraphInputs {
    demands: Vec<TargetDemand>,
    recipes: BTreeMap<i32, Recipe>,
    default_recipes: BTreeMap<i32, i32>,
    active_choices: BTreeMap<i32, i32>,
    all_craft_choices: BTreeMap<i32, i32>,
    optimizer_current: BTreeMap<i32, Option<i32>>,
    owned: BTreeMap<i32, i64>,
    excluded: BTreeSet<i32>,
    choice_map: BTreeMap<i32, CraftChoiceSnapshot>,
    all_price_ids: BTreeSet<i32>,
}

fn gather_graph(
    targets: &[TargetSnapshot],
    craft_choices: &[CraftChoiceSnapshot],
    options: CraftOptionsSnapshot,
    source: &dyn CraftDataSource,
) -> GraphInputs {
    let choice_map: BTreeMap<i32, CraftChoiceSnapshot> =
        craft_choices.iter().map(|c| (c.item_id, *c)).collect();
    let mut demands = Vec::with_capacity(targets.len());
    let mut recipes = BTreeMap::new();
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    let mut all_price_ids = BTreeSet::new();

    for target in targets {
        if target.count <= 0 {
            continue;
        }
        demands.push(TargetDemand {
            recipe_id: target.key.recipe_id,
            count: target.count,
        });
        if let Some(r) = source.recipe(target.key.recipe_id) {
            all_price_ids.insert(r.output);
            for &(child, amt) in &r.ingredients {
                if amt > 0 {
                    all_price_ids.insert(child);
                    if visited.insert(child) {
                        queue.push_back(child);
                    }
                }
            }
            recipes.insert(r.id, r);
        }
    }

    let mut default_recipes = BTreeMap::new();
    let mut active_choices = BTreeMap::new();
    let mut all_craft_choices = BTreeMap::new();
    let mut optimizer_current = BTreeMap::new();
    let mut owned = BTreeMap::new();
    let mut excluded = BTreeSet::new();

    // Walk the full reachable recipe DAG regardless of current `Buy` toggles so
    // `all_craft_choices`, `optimize_craft_choices`, and `all_price_item_ids`
    // know every candidate sub-craft.
    while let Some(item_id) = queue.pop_front() {
        all_price_ids.insert(item_id);
        if options.exclude_crystals && source.is_crystal(item_id) {
            excluded.insert(item_id);
            continue;
        }
        let explicit = choice_map.get(&item_id).copied();
        if let Some(c) = explicit
            && c.on_hand > 0
        {
            owned.insert(item_id, c.on_hand);
        }
        let default_r = source.default_recipe_for_item(item_id);
        if let Some(ref r) = default_r {
            default_recipes.insert(item_id, r.id);
        }
        let chosen_r = explicit
            .and_then(|c| c.recipe_id.and_then(|id| source.recipe(id)))
            .or(default_r);
        if let Some(r) = chosen_r {
            all_craft_choices.insert(item_id, r.id);
            if explicit.is_some_and(|c| c.mode == CraftMode::Buy) {
                optimizer_current.insert(item_id, None);
            } else {
                active_choices.insert(item_id, r.id);
                optimizer_current.insert(item_id, Some(r.id));
            }
            for &(child, amt) in &r.ingredients {
                if amt > 0 {
                    all_price_ids.insert(child);
                    if visited.insert(child) {
                        queue.push_back(child);
                    }
                }
            }
            recipes.insert(r.id, r);
        }
    }

    GraphInputs {
        demands,
        recipes,
        default_recipes,
        active_choices,
        all_craft_choices,
        optimizer_current,
        owned,
        excluded,
        choice_map,
        all_price_ids,
    }
}

fn to_offers(
    listings: &HashMap<i32, Vec<ActiveListing>>,
    item_ids: &BTreeSet<i32>,
) -> BTreeMap<i32, Vec<Offer>> {
    let mut market = BTreeMap::new();
    for &id in item_ids {
        let Some(rows) = listings.get(&id) else {
            continue;
        };
        if rows.is_empty() {
            continue;
        }
        market.insert(
            id,
            rows.iter()
                .map(|l| Offer {
                    id: l.id,
                    world: l.world_id,
                    quantity: i64::from(l.quantity),
                    price: i64::from(l.price_per_unit),
                })
                .collect(),
        );
    }
    market
}

type RowListingPair = (ListItem, Vec<ActiveListing>);

fn build_consolidated_rows(
    list_id: i32,
    raw_rows: &[RowSnapshot],
    leaf_demands: &BTreeMap<i32, i64>,
    listings: &HashMap<i32, Vec<ActiveListing>>,
) -> (Vec<RowListingPair>, Vec<RowListingPair>) {
    let mut remaining_leaves = leaf_demands.clone();
    let mut manual_rows = Vec::with_capacity(raw_rows.len());
    let mut merged_snapshots = Vec::with_capacity(raw_rows.len() + leaf_demands.len());

    for row in raw_rows {
        let price_rows = listings.get(&row.key.item_id).cloned().unwrap_or_default();
        if row.need > 0
            && let Some(item) = to_list_item(list_id, row)
        {
            manual_rows.push((item, price_rows));
        }

        let extra = if row.key.quality == Quality::Any {
            remaining_leaves.remove(&row.key.item_id).unwrap_or(0)
        } else {
            0
        };
        let combined_need = row.need.max(0).saturating_add(extra);
        if combined_need > 0 {
            merged_snapshots.push(RowSnapshot {
                key: row.key,
                need: combined_need,
                acquired: row.acquired,
                target: row.target,
            });
        }
    }

    for (item_id, need) in remaining_leaves {
        if need > 0 {
            merged_snapshots.push(RowSnapshot {
                key: RowKey::new(item_id, None),
                need,
                acquired: 0,
                target: None,
            });
        }
    }

    merged_snapshots.sort_by_key(|r| r.key);
    let consolidated_rows = merged_snapshots
        .iter()
        .filter_map(|row| {
            let item = to_list_item(list_id, row)?;
            let price_rows = listings.get(&row.key.item_id).cloned().unwrap_or_default();
            Some((item, price_rows))
        })
        .collect();

    (manual_rows, consolidated_rows)
}

fn completed_crafts_for_target(completed: i64, needed: i64, crafts: i64, yield_amount: i64) -> i64 {
    if needed <= 0 || crafts <= 0 {
        return crafts.max(0);
    }
    if completed >= needed {
        crafts
    } else {
        (completed.max(0) / yield_amount.max(1)).clamp(0, crafts)
    }
}

/// Pure craft-project state builder backed by the global game-data catalog.
pub fn build_craft_project(
    list_id: i32,
    raw_rows: &[RowSnapshot],
    targets: &[TargetSnapshot],
    craft_choices: &[CraftChoiceSnapshot],
    options: CraftOptionsSnapshot,
    listings: &HashMap<i32, Vec<ActiveListing>>,
) -> CraftProjectState {
    build_craft_project_with(
        list_id,
        raw_rows,
        targets,
        craft_choices,
        options,
        listings,
        &XivCraftDataSource,
    )
}

pub fn build_craft_project_with(
    list_id: i32,
    raw_rows: &[RowSnapshot],
    targets: &[TargetSnapshot],
    craft_choices: &[CraftChoiceSnapshot],
    options: CraftOptionsSnapshot,
    listings: &HashMap<i32, Vec<ActiveListing>>,
    source: &dyn CraftDataSource,
) -> CraftProjectState {
    if targets.is_empty() {
        let mut all_price_ids = BTreeSet::new();
        let mut manual_rows = Vec::with_capacity(raw_rows.len());
        let mut consolidated_rows = Vec::with_capacity(raw_rows.len());
        for row in raw_rows {
            all_price_ids.insert(row.key.item_id);
            let price_rows = listings.get(&row.key.item_id).cloned().unwrap_or_default();
            if let Some(item) = to_list_item(list_id, row) {
                if row.need > 0 {
                    manual_rows.push((item.clone(), price_rows.clone()));
                }
                consolidated_rows.push((item, price_rows));
            }
        }
        return CraftProjectState {
            has_targets: false,
            error: None,
            options,
            targets: Vec::new(),
            steps: Vec::new(),
            manual_rows,
            consolidated_rows,
            all_price_item_ids: all_price_ids,
            summary: CraftEstimateSummary::default(),
        };
    }

    let graph = gather_graph(targets, craft_choices, options, source);
    let mut all_price_ids = graph.all_price_ids.clone();
    for row in raw_rows {
        all_price_ids.insert(row.key.item_id);
    }

    let market = to_offers(listings, &all_price_ids);
    let vendors = source.vendor_prices(options.include_vendors);

    let expansion = match expand_many(
        &graph.demands,
        &graph.recipes,
        &graph.active_choices,
        &graph.owned,
        &graph.excluded,
        ExpandLimits::default(),
    ) {
        Ok(exp) => exp,
        Err(err) => {
            let (manual_rows, consolidated_rows) =
                build_consolidated_rows(list_id, raw_rows, &BTreeMap::new(), listings);
            return CraftProjectState {
                has_targets: true,
                error: Some(err),
                options,
                targets: Vec::new(),
                steps: Vec::new(),
                manual_rows,
                consolidated_rows,
                all_price_item_ids: all_price_ids,
                summary: CraftEstimateSummary::default(),
            };
        }
    };

    let leaf_demands: BTreeMap<i32, i64> = expansion
        .materials
        .iter()
        .filter(|m| m.remaining() > 0)
        .map(|m| (m.item, m.remaining()))
        .collect();

    let (manual_rows, consolidated_rows) =
        build_consolidated_rows(list_id, raw_rows, &leaf_demands, listings);

    // Raw acquired totals across all quality buckets per item. Shop progress for
    // craft leaves uses Quality::Any (`row.key.hq() == None`), while manual HQ/NQ
    // acquisitions still help show partial stock when available.
    let mut raw_acquired: BTreeMap<i32, i64> = BTreeMap::new();
    for row in raw_rows {
        if row.acquired > 0 && row.key.quality == Quality::Any {
            *raw_acquired.entry(row.key.item_id).or_insert(0) += row.acquired;
        }
    }

    let mut leaf_acquired_clamped = 0_i64;
    let mut leaf_needed_total = 0_i64;
    for (item_id, needed) in &leaf_demands {
        let need = *needed.max(&0);
        leaf_needed_total = leaf_needed_total.saturating_add(need);
        let acq = raw_acquired
            .get(item_id)
            .copied()
            .unwrap_or(0)
            .clamp(0, need);
        leaf_acquired_clamped = leaf_acquired_clamped.saturating_add(acq);
    }

    // Track which items are currently actively crafted intermediates.
    let active_crafted_flag: BTreeMap<i32, bool> = graph
        .active_choices
        .keys()
        .map(|item_id| (*item_id, true))
        .collect();

    let target_by_recipe: BTreeMap<i32, TargetSnapshot> =
        targets.iter().map(|t| (t.key.recipe_id, *t)).collect();

    // Accumulate ingredients already consumed by completed root crafts AND
    // completed intermediate crafts so readiness reflects remaining unconsumed stock.
    let mut consumed: BTreeMap<i32, i64> = BTreeMap::new();
    for t_mat in &expansion.targets {
        let Some(rid) = t_mat.recipe else { continue };
        let Some(snap) = target_by_recipe.get(&rid) else {
            continue;
        };
        let Some(recipe) = graph.recipes.get(&rid) else {
            continue;
        };
        let comp_crafts = completed_crafts_for_target(
            snap.completed,
            t_mat.needed,
            t_mat.crafts,
            recipe.yield_amount,
        );
        if comp_crafts > 0 {
            for &(child, per_craft) in &recipe.ingredients {
                if per_craft > 0 && !graph.excluded.contains(&child) {
                    *consumed.entry(child).or_insert(0) += comp_crafts.saturating_mul(per_craft);
                }
            }
        }
    }
    for s_mat in &expansion.steps {
        let Some(rid) = s_mat.recipe else { continue };
        let Some(recipe) = graph.recipes.get(&rid) else {
            continue;
        };
        let crafted = graph
            .choice_map
            .get(&s_mat.item)
            .map_or(0, |c| c.crafted)
            .clamp(0, s_mat.crafts);
        if crafted > 0 {
            for &(child, per_craft) in &recipe.ingredients {
                if per_craft > 0 && !graph.excluded.contains(&child) {
                    *consumed.entry(child).or_insert(0) += crafted.saturating_mul(per_craft);
                }
            }
        }
    }

    let mut remaining_acquired: BTreeMap<i32, i64> = BTreeMap::new();
    for (&item_id, &acq) in &raw_acquired {
        let used = consumed.get(&item_id).copied().unwrap_or(0);
        let rem = (acq - used).max(0);
        if rem > 0 {
            remaining_acquired.insert(item_id, rem);
        }
    }

    let mut remaining_on_hand: BTreeMap<i32, i64> = BTreeMap::new();
    let mut remaining_crafted_units: BTreeMap<i32, i64> = BTreeMap::new();
    for (&item_id, &on_hand_amt) in &graph.owned {
        let used = consumed.get(&item_id).copied().unwrap_or(0);
        remaining_on_hand.insert(item_id, (on_hand_amt - used).max(0));
    }
    for s_mat in &expansion.steps {
        let Some(rid) = s_mat.recipe else { continue };
        let yield_amount = graph.recipes.get(&rid).map_or(1, |r| r.yield_amount.max(1));
        let crafted = graph
            .choice_map
            .get(&s_mat.item)
            .map_or(0, |c| c.crafted)
            .clamp(0, s_mat.crafts);
        let crafted_total = crafted.saturating_mul(yield_amount);
        let used = consumed.get(&s_mat.item).copied().unwrap_or(0);
        let on_hand_amt = graph.owned.get(&s_mat.item).copied().unwrap_or(0);
        // Consumed units draw from on_hand first, then from crafted yield.
        let leftover_used = (used - on_hand_amt).max(0);
        let rem_crafted = (crafted_total - leftover_used).max(0);
        if rem_crafted > 0 {
            remaining_crafted_units.insert(s_mat.item, rem_crafted);
        }
    }

    // Also evaluate displayable bought intermediates that were directly pruned
    // when flipped to `Buy` (present in `expansion.materials` and also having an
    // available recipe in `graph.all_craft_choices`). Ordering: crafted steps in
    // bottom-up `expansion.steps` order first, then bought candidate steps sorted
    // cleanly by item_id (with depth info preserved where known).
    let bought_candidates: Vec<Material> = expansion
        .materials
        .iter()
        .filter(|m| m.recipe.is_none() && graph.all_craft_choices.contains_key(&m.item))
        .cloned()
        .collect();

    let eval_step_craft_cost = |recipe_id: i32, count: i64| -> Option<i64> {
        if count <= 0 {
            return Some(0);
        }
        let mut sub_choices = graph.active_choices.clone();
        if let Some(r) = graph.recipes.get(&recipe_id) {
            sub_choices.insert(r.output, recipe_id);
        }
        let sub = expand_many(
            &[TargetDemand { recipe_id, count }],
            &graph.recipes,
            &sub_choices,
            &BTreeMap::new(),
            &graph.excluded,
            ExpandLimits::default(),
        )
        .ok()?;
        let mut cost = 0_i64;
        for m in sub.materials {
            let need = m.remaining();
            if need <= 0 {
                continue;
            }
            let offers = market.get(&m.item).map(Vec::as_slice).unwrap_or(&[]);
            let p = purchase(need, offers, vendors.get(&m.item).copied());
            if p.missing() > 0 {
                return None;
            }
            cost = cost.saturating_add(p.cost);
        }
        Some(cost)
    };

    let eval_buy_cost = |item_id: i32, needed: i64| -> Option<i64> {
        if needed <= 0 {
            return Some(0);
        }
        let offers = market.get(&item_id).map(Vec::as_slice).unwrap_or(&[]);
        let p = purchase(needed, offers, vendors.get(&item_id).copied());
        (p.missing() == 0).then_some(p.cost)
    };

    let build_step_view =
        |step_number: usize, mat: &Material, mode: CraftMode| -> Option<CraftStepView> {
            let recipe_id = mat
                .recipe
                .or_else(|| graph.all_craft_choices.get(&mat.item).copied())?;
            let recipe = graph.recipes.get(&recipe_id)?;
            let (craft_type_abbr, recipe_level) = source.recipe_job_and_level(recipe_id);
            let choice = graph.choice_map.get(&mat.item).copied();
            let on_hand = choice.map_or(mat.owned, |c| c.on_hand);
            let remaining = mat.remaining().max(0);
            let yield_amount = recipe.yield_amount.max(1);
            let (crafts, surplus) = if mode == CraftMode::Buy {
                let c = if remaining > 0 {
                    (remaining + yield_amount - 1) / yield_amount
                } else {
                    0
                };
                (c, c.saturating_mul(yield_amount).saturating_sub(remaining))
            } else {
                (mat.crafts, mat.surplus)
            };
            let crafted = choice.map_or(0, |c| c.crafted).clamp(0, crafts);

            let readiness = if mode == CraftMode::Buy {
                StepReadiness::Buying
            } else {
                evaluate_step_readiness(
                    remaining,
                    crafted.saturating_mul(yield_amount),
                    crafts,
                    yield_amount,
                    recipe,
                    &graph.excluded,
                    &active_crafted_flag,
                    &remaining_acquired,
                    &remaining_crafted_units,
                    &remaining_on_hand,
                )
            };

            let consumers = expansion
                .consumers
                .get(&mat.item)
                .map(|list| {
                    list.iter()
                        .map(|c| CraftConsumerView {
                            recipe_id: c.target_recipe_id,
                            item_id: c.target_item_id,
                            name: source.item_name(c.target_item_id),
                            units_used: c.units_used,
                        })
                        .collect()
                })
                .unwrap_or_default();

            let ingredients = recipe
                .ingredients
                .iter()
                .filter(|&(_, amt)| *amt > 0)
                .map(|&(child, per_craft)| {
                    let is_crystal = source.is_crystal(child);
                    let is_crafted_step = active_crafted_flag.get(&child).copied().unwrap_or(false);
                    let needed_total = if options.exclude_crystals && is_crystal {
                        0
                    } else {
                        crafts.saturating_mul(per_craft)
                    };
                    let available = if is_crafted_step {
                        remaining_on_hand.get(&child).copied().unwrap_or(0)
                            + remaining_crafted_units.get(&child).copied().unwrap_or(0)
                    } else {
                        remaining_on_hand.get(&child).copied().unwrap_or(0)
                            + remaining_acquired.get(&child).copied().unwrap_or(0)
                    };
                    StepIngredientView {
                        item_id: child,
                        name: source.item_name(child),
                        per_craft,
                        needed_total,
                        available,
                        is_crystal,
                        is_crafted_step,
                    }
                })
                .collect();

            Some(CraftStepView {
                step_number,
                item_id: mat.item,
                item_name: source.item_name(mat.item),
                recipe_id,
                craft_type_abbr,
                recipe_level,
                needed: mat.needed,
                on_hand,
                remaining,
                crafts,
                yield_amount,
                surplus,
                crafted,
                mode,
                readiness,
                consumers,
                craft_cost: eval_step_craft_cost(recipe_id, remaining),
                buy_cost: eval_buy_cost(mat.item, remaining),
                ingredients,
            })
        };

    let mut steps = Vec::with_capacity(expansion.steps.len() + bought_candidates.len());
    for s_mat in &expansion.steps {
        let next_num = steps.len() + 1;
        if let Some(view) = build_step_view(next_num, s_mat, CraftMode::Craft) {
            steps.push(view);
        }
    }
    for b_mat in &bought_candidates {
        let next_num = steps.len() + 1;
        if let Some(view) = build_step_view(next_num, b_mat, CraftMode::Buy) {
            steps.push(view);
        }
    }

    let mut targets_view = Vec::with_capacity(expansion.targets.len());
    for t_mat in &expansion.targets {
        let Some(rid) = t_mat.recipe else { continue };
        let Some(recipe) = graph.recipes.get(&rid) else {
            continue;
        };
        let snap = target_by_recipe.get(&rid).copied();
        let count = snap.map_or(t_mat.needed, |s| s.count);
        let completed = snap.map_or(0, |s| s.completed).clamp(0, count);
        let yield_amount = recipe.yield_amount.max(1);
        let (craft_type_abbr, recipe_level) = source.recipe_job_and_level(rid);
        let readiness = evaluate_step_readiness(
            count,
            completed,
            t_mat.crafts,
            yield_amount,
            recipe,
            &graph.excluded,
            &active_crafted_flag,
            &remaining_acquired,
            &remaining_crafted_units,
            &remaining_on_hand,
        );
        targets_view.push(CraftTargetView {
            recipe_id: rid,
            item_id: t_mat.item,
            item_name: source.item_name(t_mat.item),
            craft_type_abbr,
            recipe_level,
            count,
            completed,
            crafts: t_mat.crafts,
            yield_amount,
            surplus: t_mat.surplus,
            completed_crafts: completed_crafts_for_target(
                completed,
                count,
                t_mat.crafts,
                yield_amount,
            ),
            readiness,
            buy_finished_cost: eval_buy_cost(t_mat.item, count),
        });
    }

    // Total project cost summary using exact whole-stack purchase pricing across
    // consolidated leaves vs buying finished targets.
    let mut materials_cost = 0_i64;
    let mut craft_missing_units = 0_i64;
    let mut unpriced_leaves = 0_usize;
    for (item_id, needed) in &leaf_demands {
        let offers = market.get(item_id).map(Vec::as_slice).unwrap_or(&[]);
        let p = purchase(*needed, offers, vendors.get(item_id).copied());
        if p.missing() > 0 {
            unpriced_leaves += 1;
            craft_missing_units = craft_missing_units.saturating_add(p.missing());
        }
        materials_cost = materials_cost.saturating_add(p.cost);
    }
    let materials_cost_opt =
        (unpriced_leaves < leaf_demands.len() || leaf_demands.is_empty()).then_some(materials_cost);
    let cost_summary = project_cost_summary(
        &graph.demands,
        &graph.recipes,
        materials_cost_opt,
        craft_missing_units,
        &market,
        &vendors,
    );

    let steps_done = steps
        .iter()
        .filter(|s| s.mode == CraftMode::Craft && matches!(s.readiness, StepReadiness::Done))
        .count();
    let active_steps_total = steps.iter().filter(|s| s.mode == CraftMode::Craft).count();
    let targets_done = targets_view
        .iter()
        .filter(|t| matches!(t.readiness, StepReadiness::Done))
        .count();

    CraftProjectState {
        has_targets: true,
        error: None,
        options,
        targets: targets_view,
        steps,
        manual_rows,
        consolidated_rows,
        all_price_item_ids: all_price_ids,
        summary: CraftEstimateSummary {
            materials_cost: cost_summary.craft_shopping_cost,
            buy_finished_cost: cost_summary.buy_finished_cost,
            savings: cost_summary.savings_vs_buying_finished,
            unpriced_leaves,
            steps_done,
            steps_total: active_steps_total,
            targets_done,
            targets_total: targets.len(),
            leaves_acquired: leaf_acquired_clamped,
            leaves_needed: leaf_needed_total,
        },
    }
}

/// Optimize `Craft` vs `Buy` across intermediate sub-crafts using the current
/// market listings and vendor prices.
pub fn compute_optimize_plan(
    targets: &[TargetSnapshot],
    craft_choices: &[CraftChoiceSnapshot],
    options: CraftOptionsSnapshot,
    listings: &HashMap<i32, Vec<ActiveListing>>,
) -> Result<OptimizePlan, String> {
    compute_optimize_plan_with(
        targets,
        craft_choices,
        options,
        listings,
        &XivCraftDataSource,
    )
}

pub fn compute_optimize_plan_with(
    targets: &[TargetSnapshot],
    craft_choices: &[CraftChoiceSnapshot],
    options: CraftOptionsSnapshot,
    listings: &HashMap<i32, Vec<ActiveListing>>,
    source: &dyn CraftDataSource,
) -> Result<OptimizePlan, String> {
    let graph = gather_graph(targets, craft_choices, options, source);
    let market = to_offers(listings, &graph.all_price_ids);
    let vendors = source.vendor_prices(options.include_vendors);
    optimize_craft_choices(
        &graph.demands,
        &graph.recipes,
        &graph.default_recipes,
        &graph.optimizer_current,
        &graph.owned,
        &graph.excluded,
        &market,
        &vendors,
        ExpandLimits::default(),
    )
}

pub fn format_gil(amount: i64) -> String {
    use thousands::Separable;
    amount.separate_with_commas()
}

pub fn apply_optimize_choices(
    current: &[CraftChoiceSnapshot],
    plan: &OptimizePlan,
) -> Vec<(i32, CraftMode, Option<i32>)> {
    let current_map: BTreeMap<i32, CraftChoiceSnapshot> =
        current.iter().map(|c| (c.item_id, *c)).collect();
    let mut updates = Vec::new();
    for (&item_id, &choice) in &plan.choices {
        let next_mode = if choice.is_some() {
            CraftMode::Craft
        } else {
            CraftMode::Buy
        };
        let existing = current_map.get(&item_id);
        let prev_mode = existing.map_or(CraftMode::Craft, |c| c.mode);
        let prev_recipe = existing.and_then(|c| c.recipe_id);
        if prev_mode != next_mode || (next_mode == CraftMode::Craft && prev_recipe != choice) {
            updates.push((item_id, next_mode, choice));
        }
    }
    updates
}

/// Used to retain `need == 0` purchase tracking IDs cleanly alongside Shop rows.
pub fn consolidated_effective_need(state: &CraftProjectState, key: &RowKey) -> Option<i64> {
    let id = row_id(key)?;
    state
        .consolidated_rows
        .iter()
        .find(|(item, _)| item.id == id)
        .and_then(|(item, _)| item.quantity)
        .map(i64::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_list_doc::TargetKey;

    struct FakeCatalog;
    impl CraftDataSource for FakeCatalog {
        fn recipe(&self, recipe_id: i32) -> Option<Recipe> {
            match recipe_id {
                1 => Some(Recipe {
                    id: 1,
                    output: 100,
                    yield_amount: 1,
                    ingredients: vec![(200, 1), (400, 1), (58, 2)],
                }),
                2 => Some(Recipe {
                    id: 2,
                    output: 101,
                    yield_amount: 1,
                    ingredients: vec![(200, 1), (401, 2)],
                }),
                3 => Some(Recipe {
                    id: 3,
                    output: 200,
                    yield_amount: 2,
                    ingredients: vec![(300, 3)],
                }),
                _ => None,
            }
        }
        fn default_recipe_for_item(&self, item_id: i32) -> Option<Recipe> {
            (item_id == 200).then(|| self.recipe(3)).flatten()
        }
        fn item_name(&self, item_id: i32) -> String {
            match item_id {
                100 => "Sword".into(),
                101 => "Shield".into(),
                200 => "Iron Ingot".into(),
                300 => "Iron Ore".into(),
                400 => "Leather".into(),
                401 => "Rivet".into(),
                58 => "Fire Shard".into(),
                _ => format!("Item #{item_id}"),
            }
        }
        fn is_crystal(&self, item_id: i32) -> bool {
            item_id == 58
        }
        fn recipe_job_and_level(&self, recipe_id: i32) -> (&'static str, i32) {
            match recipe_id {
                1 | 3 => ("BSM", 20),
                2 => ("ARM", 20),
                _ => ("", 0),
            }
        }
        fn vendor_prices(&self, enabled: bool) -> BTreeMap<i32, i64> {
            if enabled {
                BTreeMap::from([(300, 18)])
            } else {
                BTreeMap::new()
            }
        }
    }

    fn listing(id: i64, item_id: i32, price: i32, qty: i32) -> ActiveListing {
        ActiveListing {
            id,
            world_id: 1,
            item_id,
            hq: false,
            price_per_unit: price,
            quantity: qty,
            retainer_id: 1,
            timestamp: chrono::NaiveDateTime::default(),
        }
    }

    #[test]
    fn consolidates_shared_intermediate_hides_dormant_rows_and_tracks_readiness() {
        let rows = vec![
            // Manual extra leather + dormant Shop tracking row for ore (need = 0).
            RowSnapshot {
                key: RowKey::new(400, None),
                need: 2,
                acquired: 1,
                target: Some(500),
            },
            RowSnapshot {
                key: RowKey::new(300, None),
                need: 0,
                acquired: 3,
                target: None,
            },
        ];
        let targets = vec![
            TargetSnapshot {
                key: TargetKey::new(1),
                count: 1,
                completed: 0,
            },
            TargetSnapshot {
                key: TargetKey::new(2),
                count: 1,
                completed: 0,
            },
        ];
        let listings = HashMap::from([
            (300, vec![listing(1, 300, 10, 10)]),
            (400, vec![listing(2, 400, 100, 10)]),
            (401, vec![listing(3, 401, 50, 10)]),
            (200, vec![listing(4, 200, 500, 2)]),
            (100, vec![listing(5, 100, 800, 1)]),
            (101, vec![listing(6, 101, 800, 1)]),
        ]);

        let state = build_craft_project_with(
            1,
            &rows,
            &targets,
            &[],
            CraftOptionsSnapshot {
                exclude_crystals: true,
                include_vendors: false,
            },
            &listings,
            &FakeCatalog,
        );

        assert!(state.has_targets);
        assert!(state.error.is_none());
        // Dormant need=0 ore row stays out of Build's manual rows.
        assert_eq!(state.manual_rows.len(), 1);
        assert_eq!(state.manual_rows[0].0.item_id, 400);

        // Consolidated rows merge 2 manual leather + 1 derived leather = 3,
        // preserve target_price 500, and merge 3 acquired ores onto derived ore (need=3).
        let shop_row = |id: i32| {
            state
                .consolidated_rows
                .iter()
                .find(|(r, _)| r.item_id == id)
                .map(|(r, _)| (r.quantity.unwrap(), r.acquired.unwrap(), r.target_price))
        };
        assert_eq!(shop_row(300), Some((3, 3, None)));
        assert_eq!(shop_row(400), Some((3, 1, Some(500))));
        assert_eq!(shop_row(401), Some((2, 0, None)));
        assert_eq!(shop_row(58), None);

        // Iron Ingot step needs 2 -> 1 craft (yield 2), ready now because 3 ore acquired!
        assert_eq!(state.steps.len(), 1);
        assert_eq!(state.steps[0].crafts, 1);
        assert_eq!(
            state.steps[0].readiness,
            StepReadiness::Ready { craftable_now: 1 }
        );
        // Optimize flips Ingot to Buy only if cheaper — here Craft (30) < Buy (1000).
        let opt = compute_optimize_plan_with(
            &targets,
            &[],
            CraftOptionsSnapshot {
                exclude_crystals: true,
                include_vendors: false,
            },
            &listings,
            &FakeCatalog,
        )
        .unwrap();
        assert_eq!(opt.choices[&200], Some(3));
    }

    #[test]
    fn switching_step_to_buy_keeps_step_visible_and_updates_shop_rows() {
        let targets = vec![TargetSnapshot {
            key: TargetKey::new(1),
            count: 1,
            completed: 0,
        }];
        let choices = vec![CraftChoiceSnapshot {
            item_id: 200,
            mode: CraftMode::Buy,
            recipe_id: Some(3),
            on_hand: 0,
            crafted: 0,
        }];
        let state = build_craft_project_with(
            1,
            &[],
            &targets,
            &choices,
            CraftOptionsSnapshot {
                exclude_crystals: true,
                include_vendors: false,
            },
            &HashMap::new(),
            &FakeCatalog,
        );

        assert_eq!(state.steps.len(), 1);
        assert_eq!(state.steps[0].mode, CraftMode::Buy);
        assert_eq!(state.steps[0].readiness, StepReadiness::Buying);
        let shop_ids: Vec<i32> = state
            .consolidated_rows
            .iter()
            .map(|(r, _)| r.item_id)
            .collect();
        assert_eq!(shop_ids, vec![200, 400]);
    }
}
