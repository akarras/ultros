use serde::Deserialize;
use serde::Serialize;
use serde_aux::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

impl Recipe {
    pub fn ingredients(&self) -> impl Iterator<Item = (i64, &ItemIngredient)> {
        RecipeIngredientIterator {
            recipe: self,
            index: 0,
        }
        .flatten()
    }
}

struct RecipeIngredientIterator<'a> {
    recipe: &'a Recipe,
    index: u8,
}

impl<'a> Iterator for RecipeIngredientIterator<'a> {
    type Item = Option<(i64, &'a ItemIngredient)>;

    fn next(&mut self) -> Option<Self::Item> {
        let value = match self.index {
            0 => Some(
                self.recipe
                    .item_ingredient0
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient0, m)),
            ),
            1 => Some(
                self.recipe
                    .item_ingredient1
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient1, m)),
            ),
            2 => Some(
                self.recipe
                    .item_ingredient2
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient2, m)),
            ),
            3 => Some(
                self.recipe
                    .item_ingredient3
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient3, m)),
            ),
            4 => Some(
                self.recipe
                    .item_ingredient4
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient4, m)),
            ),
            5 => Some(
                self.recipe
                    .item_ingredient5
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient5, m)),
            ),
            6 => Some(
                self.recipe
                    .item_ingredient6
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient6, m)),
            ),
            7 => Some(
                self.recipe
                    .item_ingredient7
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient7, m)),
            ),
            8 => Some(
                self.recipe
                    .item_ingredient8
                    .as_ref()
                    .map(|m| (self.recipe.amount_ingredient8, m)),
            ),
            _ => None,
        };
        self.index += 1;
        value
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    #[serde(rename = "AmountIngredient0")]
    pub amount_ingredient0: i64,
    #[serde(rename = "AmountIngredient1")]
    pub amount_ingredient1: i64,
    #[serde(rename = "AmountIngredient2")]
    pub amount_ingredient2: i64,
    #[serde(rename = "AmountIngredient3")]
    pub amount_ingredient3: i64,
    #[serde(rename = "AmountIngredient4")]
    pub amount_ingredient4: i64,
    #[serde(rename = "AmountIngredient5")]
    pub amount_ingredient5: i64,
    #[serde(rename = "AmountIngredient6")]
    pub amount_ingredient6: i64,
    #[serde(rename = "AmountIngredient7")]
    pub amount_ingredient7: i64,
    #[serde(rename = "AmountIngredient8")]
    pub amount_ingredient8: i64,
    #[serde(rename = "AmountIngredient9")]
    pub amount_ingredient9: i64,
    #[serde(rename = "AmountResult")]
    pub amount_result: i64,
    #[serde(rename = "CanHq")]
    pub can_hq: i64,
    #[serde(rename = "CanQuickSynth")]
    pub can_quick_synth: i64,
    #[serde(rename = "ClassJob")]
    pub class_job: ClassJob,
    #[serde(rename = "CraftType")]
    pub craft_type: CraftType,
    #[serde(rename = "CraftTypeTarget")]
    pub craft_type_target: String,
    #[serde(rename = "CraftTypeTargetID")]
    pub craft_type_target_id: i64,
    #[serde(rename = "DifficultyFactor")]
    pub difficulty_factor: i64,
    #[serde(rename = "DurabilityFactor")]
    pub durability_factor: i64,
    #[serde(rename = "ExpRewarded")]
    pub exp_rewarded: i64,
    #[serde(rename = "GameContentLinks")]
    pub game_content_links: Value,
    #[serde(rename = "GamePatch")]
    pub game_patch: GamePatch,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Icon")]
    pub icon: Option<String>,
    #[serde(rename = "IconID")]
    pub icon_id: Option<i64>,
    #[serde(rename = "IsExpert")]
    pub is_expert: Option<i64>,
    #[serde(rename = "IsSecondary")]
    pub is_secondary: i64,
    #[serde(rename = "IsSpecializationRequired")]
    pub is_specialization_required: i64,
    #[serde(rename = "ItemIngredient0")]
    pub item_ingredient0: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient0Target")]
    pub item_ingredient0target: String,
    #[serde(rename = "ItemIngredient0TargetID")]
    pub item_ingredient0target_id: i64,
    #[serde(rename = "ItemIngredient1")]
    pub item_ingredient1: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient1Target")]
    pub item_ingredient1target: String,
    #[serde(
        rename = "ItemIngredient1TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient1target_id: i64,
    #[serde(rename = "ItemIngredient2")]
    pub item_ingredient2: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient2Target")]
    pub item_ingredient2target: String,
    #[serde(
        rename = "ItemIngredient2TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient2target_id: i64,
    #[serde(rename = "ItemIngredient3")]
    pub item_ingredient3: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient3Target")]
    pub item_ingredient3target: String,
    #[serde(
        rename = "ItemIngredient3TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient3target_id: i64,
    #[serde(rename = "ItemIngredient4")]
    pub item_ingredient4: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient4Target")]
    pub item_ingredient4target: String,
    #[serde(
        rename = "ItemIngredient4TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient4target_id: i64,
    #[serde(rename = "ItemIngredient5")]
    pub item_ingredient5: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient5Target")]
    pub item_ingredient5target: String,
    #[serde(
        rename = "ItemIngredient5TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient5target_id: i64,
    #[serde(rename = "ItemIngredient6")]
    pub item_ingredient6: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient6Target")]
    pub item_ingredient6target: String,
    #[serde(
        rename = "ItemIngredient6TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient6target_id: i64,
    #[serde(rename = "ItemIngredient7")]
    pub item_ingredient7: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient7Target")]
    pub item_ingredient7target: String,
    #[serde(
        rename = "ItemIngredient7TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient7target_id: i64,
    #[serde(rename = "ItemIngredient8")]
    pub item_ingredient8: Option<ItemIngredient>,
    #[serde(rename = "ItemIngredient8Target")]
    pub item_ingredient8target: String,
    #[serde(
        rename = "ItemIngredient8TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient8target_id: i64,
    #[serde(rename = "ItemIngredient9")]
    pub item_ingredient9: Value,
    #[serde(rename = "ItemIngredient9Target")]
    pub item_ingredient9target: String,
    #[serde(
        rename = "ItemIngredient9TargetID",
        deserialize_with = "deserialize_number_from_string"
    )]
    pub item_ingredient9target_id: i64,
    #[serde(rename = "ItemIngredientRecipe0")]
    pub item_ingredient_recipe0: Value,
    #[serde(rename = "ItemIngredientRecipe1")]
    pub item_ingredient_recipe1: Value,
    #[serde(rename = "ItemIngredientRecipe2")]
    pub item_ingredient_recipe2: Value,
    #[serde(rename = "ItemIngredientRecipe3")]
    pub item_ingredient_recipe3: Value,
    #[serde(rename = "ItemIngredientRecipe4")]
    pub item_ingredient_recipe4: Value,
    #[serde(rename = "ItemIngredientRecipe5")]
    pub item_ingredient_recipe5: Value,
    #[serde(rename = "ItemIngredientRecipe6")]
    pub item_ingredient_recipe6: Value,
    #[serde(rename = "ItemIngredientRecipe7")]
    pub item_ingredient_recipe7: Value,
    #[serde(rename = "ItemIngredientRecipe8")]
    pub item_ingredient_recipe8: Value,
    #[serde(rename = "ItemIngredientRecipe9")]
    pub item_ingredient_recipe9: Value,
    #[serde(rename = "ItemRequired")]
    pub item_required: Value,
    #[serde(rename = "ItemRequiredTarget")]
    pub item_required_target: String,
    #[serde(rename = "ItemRequiredTargetID")]
    pub item_required_target_id: i64,
    #[serde(rename = "ItemResult")]
    pub item_result: Option<ItemResult>,
    #[serde(rename = "ItemResultTarget")]
    pub item_result_target: String,
    #[serde(rename = "ItemResultTargetID")]
    pub item_result_target_id: i64,
    #[serde(rename = "MaterialQualityFactor")]
    pub material_quality_factor: i64,
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "Name_de")]
    pub name_de: Option<String>,
    #[serde(rename = "Name_en")]
    pub name_en: Option<String>,
    #[serde(rename = "Name_fr")]
    pub name_fr: Option<String>,
    #[serde(rename = "Name_ja")]
    pub name_ja: Option<String>,
    #[serde(rename = "Number")]
    pub number: i64,
    #[serde(rename = "Patch")]
    pub patch: i64,
    #[serde(rename = "PatchNumber")]
    pub patch_number: i64,
    #[serde(rename = "QualityFactor")]
    pub quality_factor: i64,
    #[serde(rename = "Quest")]
    pub quest: Value,
    #[serde(rename = "QuestTarget")]
    pub quest_target: String,
    #[serde(rename = "QuestTargetID")]
    pub quest_target_id: i64,
    #[serde(rename = "QuickSynthControl")]
    pub quick_synth_control: i64,
    #[serde(rename = "QuickSynthCraftsmanship")]
    pub quick_synth_craftsmanship: i64,
    #[serde(rename = "RecipeLevelTable")]
    pub recipe_level_table: Option<RecipeLevelTable>,
    #[serde(rename = "RecipeLevelTableTarget")]
    pub recipe_level_table_target: String,
    #[serde(rename = "RecipeLevelTableTargetID")]
    pub recipe_level_table_target_id: i64,
    #[serde(rename = "RecipeNotebookList")]
    pub recipe_notebook_list: i64,
    #[serde(rename = "RequiredControl")]
    pub required_control: i64,
    #[serde(rename = "RequiredCraftsmanship")]
    pub required_craftsmanship: i64,
    #[serde(rename = "SecretRecipeBook")]
    pub secret_recipe_book: Value,
    #[serde(rename = "SecretRecipeBookTarget")]
    pub secret_recipe_book_target: String,
    #[serde(rename = "SecretRecipeBookTargetID")]
    pub secret_recipe_book_target_id: i64,
    #[serde(rename = "StatusRequired")]
    pub status_required: Value,
    #[serde(rename = "StatusRequiredTarget")]
    pub status_required_target: String,
    #[serde(rename = "StatusRequiredTargetID")]
    pub status_required_target_id: i64,
    #[serde(rename = "Url")]
    pub url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassJob {
    #[serde(rename = "Abbreviation")]
    pub abbreviation: String,
    #[serde(rename = "Abbreviation_de")]
    pub abbreviation_de: String,
    #[serde(rename = "Abbreviation_en")]
    pub abbreviation_en: String,
    #[serde(rename = "Abbreviation_fr")]
    pub abbreviation_fr: String,
    #[serde(rename = "Abbreviation_ja")]
    pub abbreviation_ja: String,
    #[serde(rename = "BattleClassIndex")]
    pub battle_class_index: String,
    #[serde(rename = "CanQueueForDuty")]
    pub can_queue_for_duty: Value,
    #[serde(rename = "ClassJobCategory")]
    pub class_job_category: i64,
    #[serde(rename = "ClassJobCategoryTarget")]
    pub class_job_category_target: String,
    #[serde(rename = "ClassJobCategoryTargetID")]
    pub class_job_category_target_id: i64,
    #[serde(rename = "ClassJobParent")]
    pub class_job_parent: Value,
    #[serde(rename = "ClassJobParentTarget")]
    pub class_job_parent_target: String,
    #[serde(rename = "ClassJobParentTargetID")]
    pub class_job_parent_target_id: i64,
    #[serde(rename = "DohDolJobIndex")]
    pub doh_dol_job_index: Option<i64>,
    #[serde(rename = "ExpArrayIndex")]
    pub exp_array_index: i64,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Icon")]
    pub icon: String,
    #[serde(rename = "IsLimitedJob")]
    pub is_limited_job: Value,
    #[serde(rename = "ItemSoulCrystal")]
    pub item_soul_crystal: i64,
    #[serde(rename = "ItemSoulCrystalTarget")]
    pub item_soul_crystal_target: String,
    #[serde(rename = "ItemSoulCrystalTargetID")]
    pub item_soul_crystal_target_id: i64,
    #[serde(rename = "ItemStartingWeapon")]
    pub item_starting_weapon: Value,
    #[serde(rename = "ItemStartingWeaponTarget")]
    pub item_starting_weapon_target: String,
    #[serde(rename = "ItemStartingWeaponTargetID")]
    pub item_starting_weapon_target_id: Value,
    #[serde(rename = "JobIndex")]
    pub job_index: Value,
    #[serde(rename = "LimitBreak1")]
    pub limit_break1: Value,
    #[serde(rename = "LimitBreak1Target")]
    pub limit_break1target: String,
    #[serde(rename = "LimitBreak1TargetID")]
    pub limit_break1target_id: Value,
    #[serde(rename = "LimitBreak2")]
    pub limit_break2: Value,
    #[serde(rename = "LimitBreak2Target")]
    pub limit_break2target: String,
    #[serde(rename = "LimitBreak2TargetID")]
    pub limit_break2target_id: Value,
    #[serde(rename = "LimitBreak3")]
    pub limit_break3: Value,
    #[serde(rename = "LimitBreak3Target")]
    pub limit_break3target: String,
    #[serde(rename = "LimitBreak3TargetID")]
    pub limit_break3target_id: Value,
    #[serde(rename = "ModifierDexterity")]
    pub modifier_dexterity: i64,
    #[serde(rename = "ModifierHitPoints")]
    pub modifier_hit_points: i64,
    #[serde(rename = "ModifierIntelligence")]
    pub modifier_intelligence: i64,
    #[serde(rename = "ModifierManaPoints")]
    pub modifier_mana_points: i64,
    #[serde(rename = "ModifierMind")]
    pub modifier_mind: i64,
    #[serde(rename = "ModifierPiety")]
    pub modifier_piety: i64,
    #[serde(rename = "ModifierStrength")]
    pub modifier_strength: i64,
    #[serde(rename = "ModifierVitality")]
    pub modifier_vitality: i64,
    #[serde(rename = "MonsterNote")]
    pub monster_note: Value,
    #[serde(rename = "MonsterNoteTarget")]
    pub monster_note_target: String,
    #[serde(rename = "MonsterNoteTargetID")]
    pub monster_note_target_id: i64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "NameEnglish")]
    pub name_english: String,
    #[serde(rename = "NameEnglish_de")]
    pub name_english_de: String,
    #[serde(rename = "NameEnglish_en")]
    pub name_english_en: String,
    #[serde(rename = "NameEnglish_fr")]
    pub name_english_fr: String,
    #[serde(rename = "NameEnglish_ja")]
    pub name_english_ja: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "PartyBonus")]
    pub party_bonus: i64,
    #[serde(rename = "Prerequisite")]
    pub prerequisite: Value,
    #[serde(rename = "PrerequisiteTarget")]
    pub prerequisite_target: String,
    #[serde(rename = "PrerequisiteTargetID")]
    pub prerequisite_target_id: Value,
    #[serde(rename = "PrimaryStat")]
    pub primary_stat: Value,
    #[serde(rename = "PvPActionSortRow")]
    pub pv_paction_sort_row: Value,
    #[serde(rename = "RelicQuest")]
    pub relic_quest: Value,
    #[serde(rename = "RelicQuestTarget")]
    pub relic_quest_target: String,
    #[serde(rename = "RelicQuestTargetID")]
    pub relic_quest_target_id: Value,
    #[serde(rename = "Role")]
    pub role: Value,
    #[serde(rename = "StartingLevel")]
    pub starting_level: i64,
    #[serde(rename = "StartingTown")]
    pub starting_town: Value,
    #[serde(rename = "StartingTownTarget")]
    pub starting_town_target: String,
    #[serde(rename = "StartingTownTargetID")]
    pub starting_town_target_id: Value,
    #[serde(rename = "UIPriority")]
    pub uipriority: i64,
    #[serde(rename = "UnlockQuest")]
    pub unlock_quest: Value,
    #[serde(rename = "UnlockQuestTarget")]
    pub unlock_quest_target: String,
    #[serde(rename = "UnlockQuestTargetID")]
    pub unlock_quest_target_id: Value,
    #[serde(rename = "Url")]
    pub url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftType {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "MainPhysical")]
    pub main_physical: i64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "SubPhysical")]
    pub sub_physical: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameContentLinks {
    #[serde(rename = "RecipeLookup")]
    pub recipe_lookup: HashMap<String, Vec<i64>>,
    #[serde(rename = "RecipeNotebookList")]
    pub recipe_notebook_list: RecipeNotebookList,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeNotebookList {
    #[serde(rename = "Recipe0")]
    pub recipe0: Option<Vec<i64>>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePatch {
    #[serde(rename = "Banner")]
    pub banner: Option<String>,
    #[serde(rename = "ExName")]
    pub ex_name: String,
    #[serde(rename = "ExVersion")]
    pub ex_version: i64,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_cn")]
    pub name_cn: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "Name_kr")]
    pub name_kr: String,
    #[serde(rename = "ReleaseDate")]
    pub release_date: i64,
    #[serde(rename = "Version")]
    pub version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemIngredient {
    #[serde(rename = "AdditionalData")]
    pub additional_data: Value,
    #[serde(rename = "Adjective")]
    pub adjective: i64,
    #[serde(rename = "AetherialReduce")]
    pub aetherial_reduce: i64,
    #[serde(rename = "AlwaysCollectable")]
    pub always_collectable: i64,
    #[serde(rename = "Article")]
    pub article: i64,
    #[serde(rename = "BaseParam0")]
    pub base_param0: Value,
    #[serde(rename = "BaseParam0Target")]
    pub base_param0target: String,
    #[serde(rename = "BaseParam0TargetID")]
    pub base_param0target_id: i64,
    #[serde(rename = "BaseParam1")]
    pub base_param1: Value,
    #[serde(rename = "BaseParam1Target")]
    pub base_param1target: String,
    #[serde(rename = "BaseParam1TargetID")]
    pub base_param1target_id: i64,
    #[serde(rename = "BaseParam2")]
    pub base_param2: Value,
    #[serde(rename = "BaseParam2Target")]
    pub base_param2target: String,
    #[serde(rename = "BaseParam2TargetID")]
    pub base_param2target_id: i64,
    #[serde(rename = "BaseParam3")]
    pub base_param3: Value,
    #[serde(rename = "BaseParam3Target")]
    pub base_param3target: String,
    #[serde(rename = "BaseParam3TargetID")]
    pub base_param3target_id: i64,
    #[serde(rename = "BaseParam4")]
    pub base_param4: Value,
    #[serde(rename = "BaseParam4Target")]
    pub base_param4target: String,
    #[serde(rename = "BaseParam4TargetID")]
    pub base_param4target_id: i64,
    #[serde(rename = "BaseParam5")]
    pub base_param5: Value,
    #[serde(rename = "BaseParam5Target")]
    pub base_param5target: String,
    #[serde(rename = "BaseParam5TargetID")]
    pub base_param5target_id: i64,
    #[serde(rename = "BaseParamModifier")]
    pub base_param_modifier: i64,
    #[serde(rename = "BaseParamSpecial0")]
    pub base_param_special0: Value,
    #[serde(rename = "BaseParamSpecial0Target")]
    pub base_param_special0target: String,
    #[serde(rename = "BaseParamSpecial0TargetID")]
    pub base_param_special0target_id: i64,
    #[serde(rename = "BaseParamSpecial1")]
    pub base_param_special1: Value,
    #[serde(rename = "BaseParamSpecial1Target")]
    pub base_param_special1target: String,
    #[serde(rename = "BaseParamSpecial1TargetID")]
    pub base_param_special1target_id: i64,
    #[serde(rename = "BaseParamSpecial2")]
    pub base_param_special2: Value,
    #[serde(rename = "BaseParamSpecial2Target")]
    pub base_param_special2target: String,
    #[serde(rename = "BaseParamSpecial2TargetID")]
    pub base_param_special2target_id: i64,
    #[serde(rename = "BaseParamSpecial3")]
    pub base_param_special3: Value,
    #[serde(rename = "BaseParamSpecial3Target")]
    pub base_param_special3target: String,
    #[serde(rename = "BaseParamSpecial3TargetID")]
    pub base_param_special3target_id: i64,
    #[serde(rename = "BaseParamSpecial4")]
    pub base_param_special4: Value,
    #[serde(rename = "BaseParamSpecial4Target")]
    pub base_param_special4target: String,
    #[serde(rename = "BaseParamSpecial4TargetID")]
    pub base_param_special4target_id: i64,
    #[serde(rename = "BaseParamSpecial5")]
    pub base_param_special5: Value,
    #[serde(rename = "BaseParamSpecial5Target")]
    pub base_param_special5target: String,
    #[serde(rename = "BaseParamSpecial5TargetID")]
    pub base_param_special5target_id: i64,
    #[serde(rename = "BaseParamValue0")]
    pub base_param_value0: i64,
    #[serde(rename = "BaseParamValue1")]
    pub base_param_value1: i64,
    #[serde(rename = "BaseParamValue2")]
    pub base_param_value2: i64,
    #[serde(rename = "BaseParamValue3")]
    pub base_param_value3: i64,
    #[serde(rename = "BaseParamValue4")]
    pub base_param_value4: i64,
    #[serde(rename = "BaseParamValue5")]
    pub base_param_value5: i64,
    #[serde(rename = "BaseParamValueSpecial0")]
    pub base_param_value_special0: i64,
    #[serde(rename = "BaseParamValueSpecial1")]
    pub base_param_value_special1: i64,
    #[serde(rename = "BaseParamValueSpecial2")]
    pub base_param_value_special2: i64,
    #[serde(rename = "BaseParamValueSpecial3")]
    pub base_param_value_special3: i64,
    #[serde(rename = "BaseParamValueSpecial4")]
    pub base_param_value_special4: i64,
    #[serde(rename = "BaseParamValueSpecial5")]
    pub base_param_value_special5: i64,
    #[serde(rename = "Block")]
    pub block: i64,
    #[serde(rename = "BlockRate")]
    pub block_rate: i64,
    #[serde(rename = "CanBeHq")]
    pub can_be_hq: i64,
    #[serde(rename = "CastTimeS")]
    pub cast_time_s: i64,
    #[serde(rename = "ClassJobCategory")]
    pub class_job_category: Value,
    #[serde(rename = "ClassJobCategoryTarget")]
    pub class_job_category_target: String,
    #[serde(rename = "ClassJobCategoryTargetID")]
    pub class_job_category_target_id: i64,
    #[serde(rename = "ClassJobRepair")]
    pub class_job_repair: Value,
    #[serde(rename = "ClassJobRepairTarget")]
    pub class_job_repair_target: String,
    #[serde(rename = "ClassJobRepairTargetID")]
    pub class_job_repair_target_id: i64,
    #[serde(rename = "ClassJobUse")]
    pub class_job_use: Value,
    #[serde(rename = "ClassJobUseTarget")]
    pub class_job_use_target: String,
    #[serde(rename = "ClassJobUseTargetID")]
    pub class_job_use_target_id: i64,
    #[serde(rename = "CooldownS")]
    pub cooldown_s: i64,
    #[serde(rename = "DamageMag")]
    pub damage_mag: i64,
    #[serde(rename = "DamagePhys")]
    pub damage_phys: i64,
    #[serde(rename = "DefenseMag")]
    pub defense_mag: i64,
    #[serde(rename = "DefensePhys")]
    pub defense_phys: i64,
    #[serde(rename = "DelayMs")]
    pub delay_ms: i64,
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "Description_de")]
    pub description_de: String,
    #[serde(rename = "Description_en")]
    pub description_en: String,
    #[serde(rename = "Description_fr")]
    pub description_fr: String,
    #[serde(rename = "Description_ja")]
    pub description_ja: String,
    #[serde(rename = "Desynth")]
    pub desynth: i64,
    #[serde(rename = "EquipRestriction")]
    pub equip_restriction: i64,
    #[serde(rename = "EquipSlotCategory")]
    pub equip_slot_category: Value,
    #[serde(rename = "EquipSlotCategoryTarget")]
    pub equip_slot_category_target: String,
    #[serde(rename = "EquipSlotCategoryTargetID")]
    pub equip_slot_category_target_id: i64,
    #[serde(rename = "FilterGroup")]
    pub filter_group: i64,
    #[serde(rename = "GrandCompany")]
    pub grand_company: Value,
    #[serde(rename = "GrandCompanyTarget")]
    pub grand_company_target: String,
    #[serde(rename = "GrandCompanyTargetID")]
    pub grand_company_target_id: i64,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Icon")]
    pub icon: Option<String>,
    #[serde(rename = "IconHD")]
    pub icon_hd: Option<String>,
    #[serde(rename = "IconID")]
    pub icon_id: Option<i64>,
    #[serde(rename = "IsAdvancedMeldingPermitted")]
    pub is_advanced_melding_permitted: i64,
    #[serde(rename = "IsCollectable")]
    pub is_collectable: i64,
    #[serde(rename = "IsCrestWorthy")]
    pub is_crest_worthy: i64,
    #[serde(rename = "IsDyeable")]
    pub is_dyeable: i64,
    #[serde(rename = "IsGlamourous")]
    pub is_glamourous: i64,
    #[serde(rename = "IsIndisposable")]
    pub is_indisposable: i64,
    #[serde(rename = "IsPvP")]
    pub is_pv_p: i64,
    #[serde(rename = "IsUnique")]
    pub is_unique: i64,
    #[serde(rename = "IsUntradable")]
    pub is_untradable: i64,
    #[serde(rename = "ItemAction")]
    pub item_action: Value,
    #[serde(rename = "ItemActionTarget")]
    pub item_action_target: String,
    #[serde(rename = "ItemActionTargetID")]
    pub item_action_target_id: i64,
    #[serde(rename = "ItemGlamour")]
    pub item_glamour: Value,
    #[serde(rename = "ItemGlamourTarget")]
    pub item_glamour_target: String,
    #[serde(rename = "ItemGlamourTargetID")]
    pub item_glamour_target_id: i64,
    #[serde(rename = "ItemRepair")]
    pub item_repair: Value,
    #[serde(rename = "ItemRepairTarget")]
    pub item_repair_target: String,
    #[serde(rename = "ItemRepairTargetID")]
    pub item_repair_target_id: i64,
    #[serde(rename = "ItemSearchCategory")]
    pub item_search_category: Option<ItemSearchCategory>,
    #[serde(rename = "ItemSearchCategoryTarget")]
    pub item_search_category_target: String,
    #[serde(rename = "ItemSearchCategoryTargetID")]
    pub item_search_category_target_id: i64,
    #[serde(rename = "ItemSeries")]
    pub item_series: Value,
    #[serde(rename = "ItemSeriesTarget")]
    pub item_series_target: String,
    #[serde(rename = "ItemSeriesTargetID")]
    pub item_series_target_id: i64,
    #[serde(rename = "ItemSortCategory")]
    pub item_sort_category: ItemSortCategory,
    #[serde(rename = "ItemSortCategoryTarget")]
    pub item_sort_category_target: String,
    #[serde(rename = "ItemSortCategoryTargetID")]
    pub item_sort_category_target_id: i64,
    #[serde(rename = "ItemSpecialBonus")]
    pub item_special_bonus: Value,
    #[serde(rename = "ItemSpecialBonusParam")]
    pub item_special_bonus_param: i64,
    #[serde(rename = "ItemSpecialBonusTarget")]
    pub item_special_bonus_target: String,
    #[serde(rename = "ItemSpecialBonusTargetID")]
    pub item_special_bonus_target_id: i64,
    #[serde(rename = "ItemUICategory")]
    pub item_uicategory: ItemUicategory,
    #[serde(rename = "ItemUICategoryTarget")]
    pub item_uicategory_target: String,
    #[serde(rename = "ItemUICategoryTargetID")]
    pub item_uicategory_target_id: i64,
    #[serde(rename = "LevelEquip")]
    pub level_equip: i64,
    #[serde(rename = "LevelItem")]
    pub level_item: i64,
    #[serde(rename = "Lot")]
    pub lot: i64,
    #[serde(rename = "MateriaSlotCount")]
    pub materia_slot_count: i64,
    #[serde(rename = "MaterializeType")]
    pub materialize_type: i64,
    #[serde(rename = "ModelMain")]
    pub model_main: String,
    #[serde(rename = "ModelSub")]
    pub model_sub: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "Plural")]
    pub plural: String,
    #[serde(rename = "Plural_de")]
    pub plural_de: String,
    #[serde(rename = "Plural_en")]
    pub plural_en: String,
    #[serde(rename = "Plural_fr")]
    pub plural_fr: String,
    #[serde(rename = "Plural_ja")]
    pub plural_ja: String,
    #[serde(rename = "PossessivePronoun")]
    pub possessive_pronoun: i64,
    #[serde(rename = "PriceLow")]
    pub price_low: i64,
    #[serde(rename = "PriceMid")]
    pub price_mid: i64,
    #[serde(rename = "Pronoun")]
    pub pronoun: i64,
    #[serde(rename = "Rarity")]
    pub rarity: i64,
    #[serde(rename = "Singular")]
    pub singular: String,
    #[serde(rename = "Singular_de")]
    pub singular_de: String,
    #[serde(rename = "Singular_en")]
    pub singular_en: String,
    #[serde(rename = "Singular_fr")]
    pub singular_fr: String,
    #[serde(rename = "Singular_ja")]
    pub singular_ja: String,
    #[serde(rename = "StackSize")]
    pub stack_size: i64,
    #[serde(rename = "StartsWithVowel")]
    pub starts_with_vowel: i64,
    #[serde(rename = "SubStatCategory")]
    pub sub_stat_category: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSearchCategory {
    #[serde(rename = "Category")]
    pub category: i64,
    #[serde(rename = "ClassJob")]
    pub class_job: Value,
    #[serde(rename = "ClassJobTarget")]
    pub class_job_target: String,
    #[serde(rename = "ClassJobTargetID")]
    pub class_job_target_id: i64,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Icon")]
    pub icon: String,
    #[serde(rename = "IconHD")]
    pub icon_hd: String,
    #[serde(rename = "IconID")]
    pub icon_id: i64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "Order")]
    pub order: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSortCategory {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Param")]
    pub param: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemUicategory {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Icon")]
    pub icon: String,
    #[serde(rename = "IconHD")]
    pub icon_hd: String,
    #[serde(rename = "IconID")]
    pub icon_id: i64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "OrderMajor")]
    pub order_major: i64,
    #[serde(rename = "OrderMinor")]
    pub order_minor: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSortCategory3 {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Param")]
    pub param: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    #[serde(rename = "AdditionalData")]
    pub additional_data: Value,
    #[serde(rename = "Adjective")]
    pub adjective: i64,
    #[serde(rename = "AetherialReduce")]
    pub aetherial_reduce: i64,
    #[serde(rename = "AlwaysCollectable")]
    pub always_collectable: i64,
    #[serde(rename = "Article")]
    pub article: i64,
    #[serde(rename = "BaseParam0")]
    pub base_param0: Value,
    #[serde(rename = "BaseParam0Target")]
    pub base_param0target: String,
    #[serde(rename = "BaseParam0TargetID")]
    pub base_param0target_id: i64,
    #[serde(rename = "BaseParam1")]
    pub base_param1: Value,
    #[serde(rename = "BaseParam1Target")]
    pub base_param1target: String,
    #[serde(rename = "BaseParam1TargetID")]
    pub base_param1target_id: i64,
    #[serde(rename = "BaseParam2")]
    pub base_param2: Value,
    #[serde(rename = "BaseParam2Target")]
    pub base_param2target: String,
    #[serde(rename = "BaseParam2TargetID")]
    pub base_param2target_id: i64,
    #[serde(rename = "BaseParam3")]
    pub base_param3: Value,
    #[serde(rename = "BaseParam3Target")]
    pub base_param3target: String,
    #[serde(rename = "BaseParam3TargetID")]
    pub base_param3target_id: i64,
    #[serde(rename = "BaseParam4")]
    pub base_param4: Value,
    #[serde(rename = "BaseParam4Target")]
    pub base_param4target: String,
    #[serde(rename = "BaseParam4TargetID")]
    pub base_param4target_id: i64,
    #[serde(rename = "BaseParam5")]
    pub base_param5: Value,
    #[serde(rename = "BaseParam5Target")]
    pub base_param5target: String,
    #[serde(rename = "BaseParam5TargetID")]
    pub base_param5target_id: i64,
    #[serde(rename = "BaseParamModifier")]
    pub base_param_modifier: i64,
    #[serde(rename = "BaseParamSpecial0")]
    pub base_param_special0: Value,
    #[serde(rename = "BaseParamSpecial0Target")]
    pub base_param_special0target: String,
    #[serde(rename = "BaseParamSpecial0TargetID")]
    pub base_param_special0target_id: i64,
    #[serde(rename = "BaseParamSpecial1")]
    pub base_param_special1: Value,
    #[serde(rename = "BaseParamSpecial1Target")]
    pub base_param_special1target: String,
    #[serde(rename = "BaseParamSpecial1TargetID")]
    pub base_param_special1target_id: i64,
    #[serde(rename = "BaseParamSpecial2")]
    pub base_param_special2: Value,
    #[serde(rename = "BaseParamSpecial2Target")]
    pub base_param_special2target: String,
    #[serde(rename = "BaseParamSpecial2TargetID")]
    pub base_param_special2target_id: i64,
    #[serde(rename = "BaseParamSpecial3")]
    pub base_param_special3: Value,
    #[serde(rename = "BaseParamSpecial3Target")]
    pub base_param_special3target: String,
    #[serde(rename = "BaseParamSpecial3TargetID")]
    pub base_param_special3target_id: i64,
    #[serde(rename = "BaseParamSpecial4")]
    pub base_param_special4: Value,
    #[serde(rename = "BaseParamSpecial4Target")]
    pub base_param_special4target: String,
    #[serde(rename = "BaseParamSpecial4TargetID")]
    pub base_param_special4target_id: i64,
    #[serde(rename = "BaseParamSpecial5")]
    pub base_param_special5: Value,
    #[serde(rename = "BaseParamSpecial5Target")]
    pub base_param_special5target: String,
    #[serde(rename = "BaseParamSpecial5TargetID")]
    pub base_param_special5target_id: i64,
    #[serde(rename = "BaseParamValue0")]
    pub base_param_value0: i64,
    #[serde(rename = "BaseParamValue1")]
    pub base_param_value1: i64,
    #[serde(rename = "BaseParamValue2")]
    pub base_param_value2: i64,
    #[serde(rename = "BaseParamValue3")]
    pub base_param_value3: i64,
    #[serde(rename = "BaseParamValue4")]
    pub base_param_value4: i64,
    #[serde(rename = "BaseParamValue5")]
    pub base_param_value5: i64,
    #[serde(rename = "BaseParamValueSpecial0")]
    pub base_param_value_special0: i64,
    #[serde(rename = "BaseParamValueSpecial1")]
    pub base_param_value_special1: i64,
    #[serde(rename = "BaseParamValueSpecial2")]
    pub base_param_value_special2: i64,
    #[serde(rename = "BaseParamValueSpecial3")]
    pub base_param_value_special3: i64,
    #[serde(rename = "BaseParamValueSpecial4")]
    pub base_param_value_special4: i64,
    #[serde(rename = "BaseParamValueSpecial5")]
    pub base_param_value_special5: i64,
    #[serde(rename = "Block")]
    pub block: i64,
    #[serde(rename = "BlockRate")]
    pub block_rate: i64,
    #[serde(rename = "CanBeHq")]
    pub can_be_hq: i64,
    #[serde(rename = "CastTimeS")]
    pub cast_time_s: i64,
    #[serde(rename = "ClassJobCategory")]
    pub class_job_category: Value,
    #[serde(rename = "ClassJobCategoryTarget")]
    pub class_job_category_target: String,
    #[serde(rename = "ClassJobCategoryTargetID")]
    pub class_job_category_target_id: i64,
    #[serde(rename = "ClassJobRepair")]
    pub class_job_repair: Value,
    #[serde(rename = "ClassJobRepairTarget")]
    pub class_job_repair_target: String,
    #[serde(rename = "ClassJobRepairTargetID")]
    pub class_job_repair_target_id: i64,
    #[serde(rename = "ClassJobUse")]
    pub class_job_use: Value,
    #[serde(rename = "ClassJobUseTarget")]
    pub class_job_use_target: String,
    #[serde(rename = "ClassJobUseTargetID")]
    pub class_job_use_target_id: i64,
    #[serde(rename = "CooldownS")]
    pub cooldown_s: i64,
    #[serde(rename = "DamageMag")]
    pub damage_mag: i64,
    #[serde(rename = "DamagePhys")]
    pub damage_phys: i64,
    #[serde(rename = "DefenseMag")]
    pub defense_mag: i64,
    #[serde(rename = "DefensePhys")]
    pub defense_phys: i64,
    #[serde(rename = "DelayMs")]
    pub delay_ms: i64,
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "Description_de")]
    pub description_de: String,
    #[serde(rename = "Description_en")]
    pub description_en: String,
    #[serde(rename = "Description_fr")]
    pub description_fr: String,
    #[serde(rename = "Description_ja")]
    pub description_ja: String,
    #[serde(rename = "Desynth")]
    pub desynth: i64,
    #[serde(rename = "EquipRestriction")]
    pub equip_restriction: i64,
    #[serde(rename = "EquipSlotCategory")]
    pub equip_slot_category: Value,
    #[serde(rename = "EquipSlotCategoryTarget")]
    pub equip_slot_category_target: String,
    #[serde(rename = "EquipSlotCategoryTargetID")]
    pub equip_slot_category_target_id: i64,
    #[serde(rename = "FilterGroup")]
    pub filter_group: i64,
    #[serde(rename = "GrandCompany")]
    pub grand_company: Value,
    #[serde(rename = "GrandCompanyTarget")]
    pub grand_company_target: String,
    #[serde(rename = "GrandCompanyTargetID")]
    pub grand_company_target_id: i64,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Icon")]
    pub icon: String,
    #[serde(rename = "IconHD")]
    pub icon_hd: String,
    #[serde(rename = "IconID")]
    pub icon_id: i64,
    #[serde(rename = "IsAdvancedMeldingPermitted")]
    pub is_advanced_melding_permitted: i64,
    #[serde(rename = "IsCollectable")]
    pub is_collectable: i64,
    #[serde(rename = "IsCrestWorthy")]
    pub is_crest_worthy: i64,
    #[serde(rename = "IsDyeable")]
    pub is_dyeable: i64,
    #[serde(rename = "IsGlamourous")]
    pub is_glamourous: i64,
    #[serde(rename = "IsIndisposable")]
    pub is_indisposable: i64,
    #[serde(rename = "IsPvP")]
    pub is_pv_p: i64,
    #[serde(rename = "IsUnique")]
    pub is_unique: i64,
    #[serde(rename = "IsUntradable")]
    pub is_untradable: i64,
    #[serde(rename = "ItemAction")]
    pub item_action: Value,
    #[serde(rename = "ItemActionTarget")]
    pub item_action_target: String,
    #[serde(rename = "ItemActionTargetID")]
    pub item_action_target_id: i64,
    #[serde(rename = "ItemGlamour")]
    pub item_glamour: Value,
    #[serde(rename = "ItemGlamourTarget")]
    pub item_glamour_target: String,
    #[serde(rename = "ItemGlamourTargetID")]
    pub item_glamour_target_id: i64,
    #[serde(rename = "ItemRepair")]
    pub item_repair: Value,
    #[serde(rename = "ItemRepairTarget")]
    pub item_repair_target: String,
    #[serde(rename = "ItemRepairTargetID")]
    pub item_repair_target_id: i64,
    #[serde(rename = "ItemSearchCategory")]
    pub item_search_category: Option<ItemSearchCategory>,
    #[serde(rename = "ItemSearchCategoryTarget")]
    pub item_search_category_target: String,
    #[serde(rename = "ItemSearchCategoryTargetID")]
    pub item_search_category_target_id: i64,
    #[serde(rename = "ItemSeries")]
    pub item_series: Value,
    #[serde(rename = "ItemSeriesTarget")]
    pub item_series_target: String,
    #[serde(rename = "ItemSeriesTargetID")]
    pub item_series_target_id: i64,
    #[serde(rename = "ItemSortCategory")]
    pub item_sort_category: ItemSortCategory4,
    #[serde(rename = "ItemSortCategoryTarget")]
    pub item_sort_category_target: String,
    #[serde(rename = "ItemSortCategoryTargetID")]
    pub item_sort_category_target_id: i64,
    #[serde(rename = "ItemSpecialBonus")]
    pub item_special_bonus: Value,
    #[serde(rename = "ItemSpecialBonusParam")]
    pub item_special_bonus_param: i64,
    #[serde(rename = "ItemSpecialBonusTarget")]
    pub item_special_bonus_target: String,
    #[serde(rename = "ItemSpecialBonusTargetID")]
    pub item_special_bonus_target_id: i64,
    #[serde(rename = "ItemUICategory")]
    pub item_uicategory: ItemUicategory,
    #[serde(rename = "ItemUICategoryTarget")]
    pub item_uicategory_target: String,
    #[serde(rename = "ItemUICategoryTargetID")]
    pub item_uicategory_target_id: i64,
    #[serde(rename = "LevelEquip")]
    pub level_equip: i64,
    #[serde(rename = "LevelItem")]
    pub level_item: i64,
    #[serde(rename = "Lot")]
    pub lot: i64,
    #[serde(rename = "MateriaSlotCount")]
    pub materia_slot_count: i64,
    #[serde(rename = "MaterializeType")]
    pub materialize_type: i64,
    #[serde(rename = "ModelMain")]
    pub model_main: String,
    #[serde(rename = "ModelSub")]
    pub model_sub: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_de")]
    pub name_de: String,
    #[serde(rename = "Name_en")]
    pub name_en: String,
    #[serde(rename = "Name_fr")]
    pub name_fr: String,
    #[serde(rename = "Name_ja")]
    pub name_ja: String,
    #[serde(rename = "Plural")]
    pub plural: String,
    #[serde(rename = "Plural_de")]
    pub plural_de: String,
    #[serde(rename = "Plural_en")]
    pub plural_en: String,
    #[serde(rename = "Plural_fr")]
    pub plural_fr: String,
    #[serde(rename = "Plural_ja")]
    pub plural_ja: String,
    #[serde(rename = "PossessivePronoun")]
    pub possessive_pronoun: i64,
    #[serde(rename = "PriceLow")]
    pub price_low: i64,
    #[serde(rename = "PriceMid")]
    pub price_mid: i64,
    #[serde(rename = "Pronoun")]
    pub pronoun: i64,
    #[serde(rename = "Rarity")]
    pub rarity: i64,
    #[serde(rename = "Singular")]
    pub singular: String,
    #[serde(rename = "Singular_de")]
    pub singular_de: String,
    #[serde(rename = "Singular_en")]
    pub singular_en: String,
    #[serde(rename = "Singular_fr")]
    pub singular_fr: String,
    #[serde(rename = "Singular_ja")]
    pub singular_ja: String,
    #[serde(rename = "StackSize")]
    pub stack_size: i64,
    #[serde(rename = "StartsWithVowel")]
    pub starts_with_vowel: i64,
    #[serde(rename = "SubStatCategory")]
    pub sub_stat_category: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSortCategory4 {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Param")]
    pub param: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeLevelTable {
    #[serde(rename = "ClassJobLevel")]
    pub class_job_level: i64,
    #[serde(rename = "ConditionsFlag")]
    pub conditions_flag: i64,
    #[serde(rename = "Difficulty")]
    pub difficulty: i64,
    #[serde(rename = "Durability")]
    pub durability: i64,
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "ProgressDivider")]
    pub progress_divider: i64,
    #[serde(rename = "ProgressModifier")]
    pub progress_modifier: i64,
    #[serde(rename = "Quality")]
    pub quality: i64,
    #[serde(rename = "QualityDivider")]
    pub quality_divider: i64,
    #[serde(rename = "QualityModifier")]
    pub quality_modifier: i64,
    #[serde(rename = "Stars")]
    pub stars: i64,
    #[serde(rename = "SuggestedControl")]
    pub suggested_control: i64,
    #[serde(rename = "SuggestedCraftsmanship")]
    pub suggested_craftsmanship: i64,
}
