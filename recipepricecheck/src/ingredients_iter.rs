use serde::{Deserialize, Serialize};
use xiv_gen::ItemId;
use xiv_gen::Recipe;

pub struct IngredientsIter<'a> {
    recipe: &'a Recipe,
    ingredient_id: usize,
}

impl<'a> From<&'a Recipe> for IngredientsIter<'a> {
    fn from(r: &'a Recipe) -> Self {
        Self {
            recipe: r,
            ingredient_id: 0,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ItemIngredient {
    pub item_id: ItemId,
    pub amount: u8,
}

impl<'a> IngredientsIter<'a> {
    fn get_ingredient(&self, id: usize) -> Option<ItemIngredient> {
        let ingredient = match id {
            0 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_0(),
                amount: self.recipe.get_amount_ingredient_0(),
            },
            1 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_1(),
                amount: self.recipe.get_amount_ingredient_1(),
            },
            2 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_2(),
                amount: self.recipe.get_amount_ingredient_2(),
            },
            3 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_3(),
                amount: self.recipe.get_amount_ingredient_3(),
            },
            4 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_4(),
                amount: self.recipe.get_amount_ingredient_4(),
            },
            5 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_5(),
                amount: self.recipe.get_amount_ingredient_5(),
            },
            6 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_6(),
                amount: self.recipe.get_amount_ingredient_6(),
            },
            7 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_7(),
                amount: self.recipe.get_amount_ingredient_7(),
            },
            8 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_8(),
                amount: self.recipe.get_amount_ingredient_8(),
            },
            9 => ItemIngredient {
                item_id: self.recipe.get_item_ingredient_9(),
                amount: self.recipe.get_amount_ingredient_9(),
            },
            _ => return None,
        };
        if ingredient.item_id.inner() == 0 || ingredient.amount == 0 {
            return None;
        }
        Some(ingredient)
    }
}

impl<'a> Iterator for IngredientsIter<'a> {
    type Item = ItemIngredient;

    fn next(&mut self) -> Option<Self::Item> {
        while self.ingredient_id < 10 {
            let ingredient = self.get_ingredient(self.ingredient_id);
            self.ingredient_id += 1;
            if ingredient.is_some() {
                return ingredient;
            }
        }
        None
    }
}
