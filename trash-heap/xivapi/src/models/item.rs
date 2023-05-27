use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Item {
    #[serde(rename = "ID")]
    id: i32,
    name: String,
    description: String,
    rarity: i8,
    plural: String,
    recipes: Vec<ItemRecipes>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemRecipes {
    #[serde(rename = "ClassJobID")]
    class_job_id: i8,
    recipe_id: i32,
    level: i8,
}
