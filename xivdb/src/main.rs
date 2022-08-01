use bonsaidb::core::connection::{
    AsyncLowLevelConnection, AsyncStorageConnection, StorageConnection,
};
use bonsaidb::core::permissions::Permissions;
use bonsaidb::core::schema::Schema;
use bonsaidb::core::schema::{Collection, SerializedCollection};
use bonsaidb::local::config::{Builder, StorageConfiguration};
use bonsaidb::local::{AsyncDatabase, AsyncStorage, Storage};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::sync::Arc;
use xivapi::RecipeRequest;
use xivdb::{Ingredient, Recipe, XivSchema};

#[tokio::main]
async fn main() -> Result<(), bonsaidb::core::Error> {
    let recipes =
        AsyncDatabase::open::<XivSchema>(StorageConfiguration::new("xiv.bonsaidb")).await?;

    //    let recipes = storage.create_database::<XivSchema>("xivdata", true).await?;
    let paths: Vec<_> = std::fs::read_dir("./Recipe")
        .unwrap()
        .map(|m| m.unwrap().path())
        .collect();
    let recipe_dir = paths.iter().map(|m| async {
        let string = tokio::fs::read_to_string(m).await.unwrap();
        let json: xivapi::models::recipe::Recipe = serde_json::from_str(&string).unwrap();
        Recipe {
            id: json.id as u32,
            target_id: json.item_result_target_id as u32,
            ingredients: json
                .ingredients()
                .map(|(amount, id)| Ingredient {
                    amount: amount as i32,
                    id: id.id as u32,
                })
                .collect(),
            name: json.name?,
            name_de: json.name_de?,
            name_ja: json.name_ja?,
            name_fr: json.name_fr?,
        }
        .push_into_async(&recipes)
        .await
        .unwrap();
        Some(())
    });
    let value: Vec<Option<()>> = futures::future::join_all(recipe_dir).await;
    Ok(())
}
