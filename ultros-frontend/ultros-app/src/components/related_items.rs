/// Related items links items that are related to the current set
use leptos::*;
use xiv_gen::{Item, ItemId, Recipe};

use super::{cheapest_price::*, small_item_display::*};

/// This iterator will attempt to find related items using the classjobcategory && ilvl
fn item_set_iter(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &xiv_gen_db::decompress_data().items;
    items.values().filter(|i| {
        item.class_job_category.0 != 0
            && item.class_job_category.0 == i.class_job_category.0
            && item.level_item.0 == i.level_item.0
            && i.key_id != item.key_id
            && item.item_search_category.0 != 0
    })
}

/// Creates an iterator over the ingredients in a recipe
struct IngredientsIter(&'static Recipe, u8);
impl IngredientsIter {
    fn new(recipe: &'static Recipe) -> Self {
        Self(recipe, 0)
    }
}
impl Iterator for IngredientsIter {
    type Item = (ItemId, u8);

    fn next(&mut self) -> Option<Self::Item> {
        // I don't remember entirely if the ingredients all are in order.
        loop {
            let counter = self.1;
            let id = match counter {
                0 => (self.0.item_ingredient_0, self.0.amount_ingredient_0),
                1 => (self.0.item_ingredient_1, self.0.amount_ingredient_1),
                2 => (self.0.item_ingredient_2, self.0.amount_ingredient_2),
                3 => (self.0.item_ingredient_3, self.0.amount_ingredient_3),
                4 => (self.0.item_ingredient_4, self.0.amount_ingredient_4),
                5 => (self.0.item_ingredient_5, self.0.amount_ingredient_5),
                6 => (self.0.item_ingredient_6, self.0.amount_ingredient_6),
                7 => (self.0.item_ingredient_7, self.0.amount_ingredient_7),
                8 => (self.0.item_ingredient_8, self.0.amount_ingredient_8),
                9 => (self.0.item_ingredient_9, self.0.amount_ingredient_9),
                _ => return None,
            };
            self.1 += 1;
            // check if this is a valid id
            if id.0 .0 != 0 {
                return Some(id);
            }
        }
    }
}

/// This iterator will traverse the recipe tree for items that are related to using this item for crafting
fn recipe_tree_iter(item_id: ItemId) -> impl Iterator<Item = &'static Recipe> {
    let recipes = &xiv_gen_db::decompress_data().recipes;
    // our item id could be in item_result, or item_ingredient
    recipes.values().filter(move |filter| {
        filter.item_result == item_id
            || IngredientsIter::new(filter).any(|(i, _amount)| i.0 == item_id.0)
    })
}

#[component]
fn Recipe(cx: Scope, recipe: &'static Recipe) -> impl IntoView {
    let items = &xiv_gen_db::decompress_data().items;
    let ingredients = IngredientsIter::new(recipe)
        .flat_map(|(ingredient, amount)| items.get(&ingredient).map(|item| (item, amount)))
        .map(|(ingredient, amount)| view! {cx,
            <div class="flex-row">
                <span style="color:#dab;">{amount.to_string()}</span>"x"<SmallItemDisplay item=ingredient/>
                <CheapestPrice item_id=ingredient.key_id hq=None />
            </div>})
        .collect::<Vec<_>>();
    let target_item = items.get(&recipe.item_result)?;
    Some(view! {cx, <div>
        "Crafting Recipe:"
        <div class="flex-row"><SmallItemDisplay item=target_item/><CheapestPrice item_id=target_item.key_id hq=None /></div>
        "Ingredients:"
        {ingredients}
        <div class="flex-row">"Optimistic price to craft: "</div>
    </div>})
}

#[component]
pub fn RelatedItems(cx: Scope, item_id: ItemId) -> impl IntoView {
    let db = xiv_gen_db::decompress_data();
    let item = db.items.get(&item_id);
    item.map(|item| {
        let item_set = item_set_iter(item)
            .map(|item| {
                view! {cx,
                    <SmallItemDisplay item/>
                }
            })
            .take(8)
            .collect::<Vec<_>>();
        let recipes = recipe_tree_iter(item_id)
            .map(|recipe| view! {cx, <Recipe recipe/>})
            .take(10)
            .collect::<Vec<_>>();
        view! {cx,
            {(!item_set.is_empty()).then(|| {
                view!{cx, <div class="content-well flex-column">
                <span class="content-title">"related items"</span>
                <div class="flex-wrap">{item_set}</div>
            </div>}
            })}
        {(!recipes.is_empty()).then(|| {
            view!{cx, <div class="content-well flex-column">
            <span class="content-title">"crafting recipes"</span>
            <div class="flex-wrap">{recipes}</div>
        </div>}
        })}
        }
    })
}
