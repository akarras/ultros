use bonsaidb::core::document::Emit;
use bonsaidb::{
    core::document::BorrowedDocument,
    core::schema::{ViewMapResult, ViewSchema},
    core::{
        connection::{AsyncLowLevelConnection, AsyncStorageConnection, StorageConnection},
        permissions::Permissions,
        schema::Schema,
        schema::{Collection, SerializedCollection, SerializedView, View},
    },
    local::{
        config::{Builder, StorageConfiguration},
        AsyncDatabase, AsyncStorage, Storage,
    },
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use xivapi::RecipeRequest;

#[derive(Debug, Schema)]
#[schema(name = "xiv-data", collections = [Recipe])]
pub struct XivSchema;

/*#[derive(Debug, Clone, Deserialize, Serialize, Collection)]
#[collection(name = "items", primary_key = u32, natural_id = |item: &Item| Some(item.id), views = [ItemByName])]
pub struct Item {
    id: u32,
    name: String,

}*/

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ingredient {
    pub amount: i32,
    pub id: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Collection)]
#[collection(name = "recipes", primary_key = u32, natural_id = |recipe: &Recipe| Some(recipe.id), views = [RecipesByName])]
pub struct Recipe {
    pub id: u32,
    pub target_id: u32,
    pub name: String,
    pub name_de: String,
    pub name_ja: String,
    pub name_fr: String,
    pub ingredients: Vec<Ingredient>,
}

#[derive(Debug, Clone, View)]
#[view(collection = Recipe, key = String, value = String, name = "by-name")]
pub struct RecipesByName;

impl ViewSchema for RecipesByName {
    type View = Self;

    fn map(&self, document: &BorrowedDocument<'_>) -> ViewMapResult<Self::View> {
        let recipe = Recipe::document_contents(document)?;

        recipe
            .name
            .to_lowercase()
            .split(' ')
            .map(|m| {
                document
                    .header
                    .emit_key_and_value(m.to_string(), "English".to_string())
            })
            .collect()
    }
}

pub struct RecipeDatabaseWrapper {
    database: AsyncDatabase,
}

impl RecipeDatabaseWrapper {
    pub async fn try_new_async() -> Result<Self, bonsaidb::core::Error> {
        let database =
            AsyncDatabase::open::<XivSchema>(StorageConfiguration::new("xiv.bonsaidb")).await?;
        Ok(Self { database })
    }

    pub async fn search_recipe_by_name(&self, name_str: &str) -> Vec<Recipe> {
        let value = &RecipesByName::entries_async(&self.database)
            .with_key(&name_str.to_lowercase())
            .query_with_collection_docs()
            .await
            .unwrap();
        let docs: Vec<_> = value
            .documents
            .values()
            .map(|m| m.contents.clone())
            .collect();
        docs
    }
}

#[cfg(test)]
mod test {
    use crate::RecipeDatabaseWrapper;

    #[tokio::test]
    async fn recipe() {
        let test = RecipeDatabaseWrapper::try_new_async().await.unwrap();
        assert!(test.search_recipe_by_name("Bronze").await.len() > 0);
    }
}
