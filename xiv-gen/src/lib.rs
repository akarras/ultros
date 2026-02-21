#![allow(unused_imports, dead_code)]

#[cfg(feature = "csv_to_bincode")]
pub mod csv_to_bincode;

mod deserialize_custom;
pub mod subrow_key;

use bincode::{Decode, Encode, config::Config};
use deserialize_custom::*;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use xiv_gen_macros::FromCsv;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub enum Language {
    En,
    Ja,
    De,
    Fr,
    Cn,
    Ko,
}

impl Language {
    pub fn to_path_part(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Ja => "ja",
            Language::De => "de",
            Language::Fr => "fr",
            Language::Cn => "cn",
            Language::Ko => "ko",
        }
    }
}

pub fn bincode_config() -> impl Config {
    bincode::config::standard()
}

pub fn data_version() -> &'static str {
    env!("GIT_HASH")
}

pub trait FromCsv {
    fn from_csv_row(header: &[String], row: &csv::StringRecord) -> Self;
}

pub trait HasId {
    type Id;
    fn get_id(&self) -> Self::Id;
}

// Define ID types
macro_rules! define_id {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            Serialize,
            Deserialize,
            PartialEq,
            Eq,
            Hash,
            Encode,
            Decode,
            derive_more::FromStr,
            Default,
        )]
        pub struct $name(pub i32);
    };
}

define_id!(ItemId);
define_id!(RecipeId);
define_id!(ClassJobId);
define_id!(ClassJobCategoryId);
define_id!(BaseParamId);
define_id!(ItemUiCategoryId);
define_id!(ItemSearchCategoryId);
define_id!(ItemSortCategoryId);
define_id!(GilShopId);
define_id!(SpecialShopId);
define_id!(RetainerTaskId);
define_id!(RetainerTaskNormalId);
define_id!(RecipeLevelTableId);
define_id!(CollectablesShopItemId);
define_id!(CollectablesShopRewardScripId);
define_id!(CraftLeveId);
define_id!(TopicSelectId);
define_id!(PreHandlerId);
define_id!(LeveId);
define_id!(LeveRewardItemId);
define_id!(LeveRewardItemGroupId);
define_id!(ENpcBaseId);
define_id!(ENpcResidentId);
define_id!(CompanyCraftSequenceId);
define_id!(CompanyCraftPartId);
define_id!(CompanyCraftProcessId);
define_id!(CompanyCraftSupplyItemId);
define_id!(CompanyCraftDraftCategoryId);
define_id!(CompanyCraftTypeId);
define_id!(CompanyCraftDraftId);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "Item")]
pub struct Item {
    #[xiv_gen(column = "#")]
    pub key_id: ItemId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "Description")]
    pub description: String,
    #[xiv_gen(column = "Icon")]
    pub icon: i32,
    #[xiv_gen(column = "ItemUICategory")]
    pub item_ui_category: i32,
    #[xiv_gen(column = "ItemSearchCategory")]
    pub item_search_category: i32,
    #[xiv_gen(column = "BaseParam[{}]", count = 6)]
    pub base_param: [u8; 6],
    #[xiv_gen(column = "BaseParamValue[{}]", count = 6)]
    pub base_param_value: [i16; 6],
    #[xiv_gen(column = "BaseParamSpecial[{}]", count = 6)]
    pub base_param_special: [u8; 6],
    #[xiv_gen(column = "BaseParamValueSpecial[{}]", count = 6)]
    pub base_param_value_special: [i16; 6],
    #[xiv_gen(column = "ItemSortCategory")]
    pub item_sort_category: i32,
    #[xiv_gen(column = "LevelItem")]
    pub level_item: i32,
    #[xiv_gen(column = "LevelEquip")]
    pub level_equip: i32,
    #[xiv_gen(column = "CanBeHq")]
    pub can_be_hq: bool,
    #[xiv_gen(column = "IsCollectable")]
    pub is_collectable: bool,
    #[xiv_gen(column = "PriceMid")]
    pub price_mid: u32,
    #[xiv_gen(column = "PriceLow")]
    pub price_low: u32,
    #[xiv_gen(column = "StackSize")]
    pub stack_size: u32,
    #[xiv_gen(column = "ClassJobCategory")]
    pub class_job_category: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "Recipe")]
pub struct Recipe {
    #[xiv_gen(column = "#")]
    pub key_id: RecipeId,
    #[xiv_gen(column = "ItemResult")]
    pub item_result: i32,
    #[xiv_gen(column = "AmountResult")]
    pub amount_result: i32,
    #[xiv_gen(column = "Ingredient[{}]", count = 8)]
    pub ingredient: [i32; 8],
    #[xiv_gen(column = "AmountIngredient[{}]", count = 8)]
    pub amount_ingredient: [i32; 8],
    #[xiv_gen(column = "CraftType")]
    pub craft_type: i32,
    #[xiv_gen(column = "RecipeLevelTable")]
    pub recipe_level_table: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ClassJob")]
pub struct ClassJob {
    #[xiv_gen(column = "#")]
    pub key_id: ClassJobId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "Abbreviation")]
    pub abbreviation: String,
    #[xiv_gen(column = "JobIndex")]
    pub job_index: i8,
    #[xiv_gen(column = "DohDolJobIndex")]
    pub doh_dol_job_index: i8,
    #[xiv_gen(column = "UIPriority")]
    pub ui_priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ClassJobCategory")]
pub struct ClassJobCategory {
    #[xiv_gen(column = "#")]
    pub key_id: ClassJobCategoryId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "ADV")]
    pub adv: bool,
    #[xiv_gen(column = "GLA")]
    pub gla: bool,
    #[xiv_gen(column = "PGL")]
    pub pgl: bool,
    #[xiv_gen(column = "MRD")]
    pub mrd: bool,
    #[xiv_gen(column = "LNC")]
    pub lnc: bool,
    #[xiv_gen(column = "ARC")]
    pub arc: bool,
    #[xiv_gen(column = "CNJ")]
    pub cnj: bool,
    #[xiv_gen(column = "THM")]
    pub thm: bool,
    #[xiv_gen(column = "CRP")]
    pub crp: bool,
    #[xiv_gen(column = "BSM")]
    pub bsm: bool,
    #[xiv_gen(column = "ARM")]
    pub arm: bool,
    #[xiv_gen(column = "GSM")]
    pub gsm: bool,
    #[xiv_gen(column = "LTW")]
    pub ltw: bool,
    #[xiv_gen(column = "WVR")]
    pub wvr: bool,
    #[xiv_gen(column = "ALC")]
    pub alc: bool,
    #[xiv_gen(column = "CUL")]
    pub cul: bool,
    #[xiv_gen(column = "MIN")]
    pub min: bool,
    #[xiv_gen(column = "BTN")]
    pub btn: bool,
    #[xiv_gen(column = "FSH")]
    pub fsh: bool,
    #[xiv_gen(column = "PLD")]
    pub pld: bool,
    #[xiv_gen(column = "MNK")]
    pub mnk: bool,
    #[xiv_gen(column = "WAR")]
    pub war: bool,
    #[xiv_gen(column = "DRG")]
    pub drg: bool,
    #[xiv_gen(column = "BRD")]
    pub brd: bool,
    #[xiv_gen(column = "WHM")]
    pub whm: bool,
    #[xiv_gen(column = "BLM")]
    pub blm: bool,
    #[xiv_gen(column = "ACN")]
    pub acn: bool,
    #[xiv_gen(column = "SMN")]
    pub smn: bool,
    #[xiv_gen(column = "SCH")]
    pub sch: bool,
    #[xiv_gen(column = "ROG")]
    pub rog: bool,
    #[xiv_gen(column = "NIN")]
    pub nin: bool,
    #[xiv_gen(column = "MCH")]
    pub mch: bool,
    #[xiv_gen(column = "DRK")]
    pub drk: bool,
    #[xiv_gen(column = "AST")]
    pub ast: bool,
    #[xiv_gen(column = "SAM")]
    pub sam: bool,
    #[xiv_gen(column = "RDM")]
    pub rdm: bool,
    #[xiv_gen(column = "BLU")]
    pub blu: bool,
    #[xiv_gen(column = "GNB")]
    pub gnb: bool,
    #[xiv_gen(column = "DNC")]
    pub dnc: bool,
    #[xiv_gen(column = "RPR")]
    pub rpr: bool,
    #[xiv_gen(column = "SGE")]
    pub sge: bool,
    #[xiv_gen(column = "VPR")]
    pub vpr: bool,
    #[xiv_gen(column = "PCT")]
    pub pct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "BaseParam")]
pub struct BaseParam {
    #[xiv_gen(column = "#")]
    pub key_id: BaseParamId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "Description")]
    pub description: String,
    #[xiv_gen(column = "OrderPriority")]
    pub order_priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "SpecialShop")]
pub struct SpecialShop {
    #[xiv_gen(column = "#")]
    pub key_id: SpecialShopId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "Item[{}].Item[0]", count = 60)]
    pub item: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "Leve")]
pub struct Leve {
    #[xiv_gen(column = "#")]
    pub key_id: LeveId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "LeveRewardItem")]
    pub leve_reward_item: i32,
    #[xiv_gen(column = "ClassJobCategory")]
    pub class_job_category: i32,
    #[xiv_gen(column = "ClassJobLevel")]
    pub class_job_level: i8,
    #[xiv_gen(column = "GilReward")]
    pub gil_reward: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "LeveRewardItem")]
pub struct LeveRewardItem {
    #[xiv_gen(column = "#")]
    pub key_id: LeveRewardItemId,
    #[xiv_gen(column = "LeveRewardItemGroup[{}]", count = 8)]
    pub leve_reward_item_group: [u16; 8],
    // Add these fields because they are used in related_items.rs
    #[xiv_gen(column = "LeveRewardItemGroup[0]")]
    pub leve_reward_item_group_0: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[1]")]
    pub leve_reward_item_group_1: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[2]")]
    pub leve_reward_item_group_2: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[3]")]
    pub leve_reward_item_group_3: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[4]")]
    pub leve_reward_item_group_4: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[5]")]
    pub leve_reward_item_group_5: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[6]")]
    pub leve_reward_item_group_6: u16,
    #[xiv_gen(column = "LeveRewardItemGroup[7]")]
    pub leve_reward_item_group_7: u16,
    #[xiv_gen(column = "ProbabilityPercent[0]")]
    pub probability_percent_0: u8,
    #[xiv_gen(column = "ProbabilityPercent[1]")]
    pub probability_percent_1: u8,
    #[xiv_gen(column = "ProbabilityPercent[2]")]
    pub probability_percent_2: u8,
    #[xiv_gen(column = "ProbabilityPercent[3]")]
    pub probability_percent_3: u8,
    #[xiv_gen(column = "ProbabilityPercent[4]")]
    pub probability_percent_4: u8,
    #[xiv_gen(column = "ProbabilityPercent[5]")]
    pub probability_percent_5: u8,
    #[xiv_gen(column = "ProbabilityPercent[6]")]
    pub probability_percent_6: u8,
    #[xiv_gen(column = "ProbabilityPercent[7]")]
    pub probability_percent_7: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "LeveRewardItemGroup")]
pub struct LeveRewardItemGroup {
    #[xiv_gen(column = "#")]
    pub key_id: LeveRewardItemGroupId,
    #[xiv_gen(column = "Item[{}]", count = 9)]
    pub item: [u16; 9],
    // Add individual fields for convenience
    #[xiv_gen(column = "Item[0]")]
    pub item_0: u16,
    #[xiv_gen(column = "Item[1]")]
    pub item_1: u16,
    #[xiv_gen(column = "Item[2]")]
    pub item_2: u16,
    #[xiv_gen(column = "Item[3]")]
    pub item_3: u16,
    #[xiv_gen(column = "Item[4]")]
    pub item_4: u16,
    #[xiv_gen(column = "Item[5]")]
    pub item_5: u16,
    #[xiv_gen(column = "Item[6]")]
    pub item_6: u16,
    #[xiv_gen(column = "Item[7]")]
    pub item_7: u16,
    #[xiv_gen(column = "Item[8]")]
    pub item_8: u16,
    #[xiv_gen(column = "Count[0]")]
    pub count_0: u8,
    #[xiv_gen(column = "Count[1]")]
    pub count_1: u8,
    #[xiv_gen(column = "Count[2]")]
    pub count_2: u8,
    #[xiv_gen(column = "Count[3]")]
    pub count_3: u8,
    #[xiv_gen(column = "Count[4]")]
    pub count_4: u8,
    #[xiv_gen(column = "Count[5]")]
    pub count_5: u8,
    #[xiv_gen(column = "Count[6]")]
    pub count_6: u8,
    #[xiv_gen(column = "Count[7]")]
    pub count_7: u8,
    #[xiv_gen(column = "Count[8]")]
    pub count_8: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ENpcBase")]
pub struct ENpcBase {
    #[xiv_gen(column = "#")]
    pub key_id: ENpcBaseId,
    #[xiv_gen(column = "ENpcData[{}]", count = 32)]
    pub e_npc_data: [u32; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ENpcResident")]
pub struct ENpcResident {
    #[xiv_gen(column = "#")]
    pub key_id: ENpcResidentId,
    #[xiv_gen(column = "Singular")]
    pub singular: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "GilShop")]
pub struct GilShop {
    #[xiv_gen(column = "#")]
    pub key_id: GilShopId,
    #[xiv_gen(column = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "GilShopItem")]
pub struct GilShopItem {
    #[xiv_gen(column = "#")]
    pub key_id: crate::subrow_key::SubrowKey<GilShopId>,
    #[xiv_gen(column = "Item")]
    pub item: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "TopicSelect")]
pub struct TopicSelect {
    #[xiv_gen(column = "#")]
    pub key_id: TopicSelectId,
    #[xiv_gen(column = "Shop[{}]", count = 10)]
    pub shop: [i32; 10],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "PreHandler")]
pub struct PreHandler {
    #[xiv_gen(column = "#")]
    pub key_id: PreHandlerId,
    #[xiv_gen(column = "Target")]
    pub target: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ItemSearchCategory")]
pub struct ItemSearchCategory {
    #[xiv_gen(column = "#")]
    pub key_id: ItemSearchCategoryId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "Category")]
    pub category: u8,
    #[xiv_gen(column = "Order")]
    pub order: u8,
    #[xiv_gen(column = "ClassJob")]
    pub class_job: i8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ItemUICategory")]
pub struct ItemUiCategory {
    #[xiv_gen(column = "#")]
    pub key_id: ItemUiCategoryId,
    #[xiv_gen(column = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "ItemSortCategory")]
pub struct ItemSortCategory {
    #[xiv_gen(column = "#")]
    pub key_id: ItemSortCategoryId,
    #[xiv_gen(column = "Param")]
    pub param: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftSequence")]
pub struct CompanyCraftSequence {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftSequenceId,
    #[xiv_gen(column = "ResultItem")]
    pub result_item: i32,
    #[xiv_gen(column = "Category")]
    pub category: i32,
    #[xiv_gen(column = "CompanyCraftPart[{}]", count = 8)]
    pub company_craft_part: [i32; 8],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftPart")]
pub struct CompanyCraftPart {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftPartId,
    #[xiv_gen(column = "CompanyCraftProcess[{}]", count = 3)]
    pub company_craft_process: [i32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftProcess")]
pub struct CompanyCraftProcess {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftProcessId,
    #[xiv_gen(column = "SupplyItem[{}]", count = 12)]
    pub supply_item: [i32; 12],
    #[xiv_gen(column = "SetQuantity[{}]", count = 12)]
    pub set_quantity: [i32; 12],
    #[xiv_gen(column = "SetsRequired[{}]", count = 12)]
    pub sets_required: [i32; 12],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftSupplyItem")]
pub struct CompanyCraftSupplyItem {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftSupplyItemId,
    #[xiv_gen(column = "Item")]
    pub item: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftDraftCategory")]
pub struct CompanyCraftDraftCategory {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftDraftCategoryId,
    #[xiv_gen(column = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftType")]
pub struct CompanyCraftType {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftTypeId,
    #[xiv_gen(column = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CompanyCraftDraft")]
pub struct CompanyCraftDraft {
    #[xiv_gen(column = "#")]
    pub key_id: CompanyCraftDraftId,
    #[xiv_gen(column = "Name")]
    pub name: String,
    #[xiv_gen(column = "RequiredItem[{}]", count = 3)]
    pub required_item: [i32; 3],
    #[xiv_gen(column = "RequiredItemCount[{}]", count = 3)]
    pub required_item_count: [i32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "RetainerTask")]
pub struct RetainerTask {
    #[xiv_gen(column = "#")]
    pub key_id: RetainerTaskId,
    #[xiv_gen(column = "Task")]
    pub task: i32,
    #[xiv_gen(column = "ClassJobCategory")]
    pub class_job_category: i32,
    #[xiv_gen(column = "RetainerLevel")]
    pub retainer_level: u8,
    #[xiv_gen(column = "IsRandom")]
    pub is_random: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "RetainerTaskNormal")]
pub struct RetainerTaskNormal {
    #[xiv_gen(column = "#")]
    pub key_id: RetainerTaskNormalId,
    #[xiv_gen(column = "Item")]
    pub item: i32,
    #[xiv_gen(column = "Quantity[0]")]
    pub quantity_0: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "RecipeLevelTable")]
pub struct RecipeLevelTable {
    #[xiv_gen(column = "#")]
    pub key_id: RecipeLevelTableId,
    #[xiv_gen(column = "ClassJobLevel")]
    pub class_job_level: i8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CollectablesShopItem")]
pub struct CollectablesShopItem {
    #[xiv_gen(column = "#")]
    pub key_id: crate::subrow_key::SubrowKey<i32>,
    #[xiv_gen(column = "Item")]
    pub item: i32,
    #[xiv_gen(column = "CollectablesShopRewardScrip")]
    pub collectables_shop_reward_scrip: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CollectablesShopRewardScrip")]
pub struct CollectablesShopRewardScrip {
    #[xiv_gen(column = "#")]
    pub key_id: CollectablesShopRewardScripId,
    #[xiv_gen(column = "Currency")]
    pub currency: i32,
    #[xiv_gen(column = "HighReward")]
    pub high_reward: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, FromCsv)]
#[xiv_gen(sheet = "CraftLeve")]
pub struct CraftLeve {
    #[xiv_gen(column = "#")]
    pub key_id: CraftLeveId,
    #[xiv_gen(column = "Leve")]
    pub leve: i32,
    #[xiv_gen(column = "Item[0]")]
    pub item_0: i32,
    #[xiv_gen(column = "ItemCount[0]")]
    pub item_count_0: i8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, Default)]
pub struct Data {
    pub items: HashMap<ItemId, Item>,
    pub recipes: HashMap<RecipeId, Recipe>,
    pub class_jobs: HashMap<ClassJobId, ClassJob>,
    pub class_job_categorys: HashMap<ClassJobCategoryId, ClassJobCategory>,
    pub base_params: HashMap<BaseParamId, BaseParam>,
    pub special_shops: HashMap<SpecialShopId, SpecialShop>,
    pub leves: HashMap<LeveId, Leve>,
    pub leve_reward_items: HashMap<LeveRewardItemId, LeveRewardItem>,
    pub leve_reward_item_groups: HashMap<LeveRewardItemGroupId, LeveRewardItemGroup>,
    pub e_npc_bases: HashMap<ENpcBaseId, ENpcBase>,
    pub e_npc_residents: HashMap<ENpcResidentId, ENpcResident>,
    pub gil_shops: HashMap<GilShopId, GilShop>,
    pub gil_shop_items: HashMap<GilShopId, Vec<GilShopItem>>,
    pub topic_selects: HashMap<TopicSelectId, TopicSelect>,
    pub pre_handlers: HashMap<PreHandlerId, PreHandler>,
    pub item_search_categorys: HashMap<ItemSearchCategoryId, ItemSearchCategory>,
    pub item_ui_categorys: HashMap<ItemUiCategoryId, ItemUiCategory>,
    pub item_sort_categorys: HashMap<ItemSortCategoryId, ItemSortCategory>,
    pub company_craft_sequences: HashMap<CompanyCraftSequenceId, CompanyCraftSequence>,
    pub company_craft_parts: HashMap<CompanyCraftPartId, CompanyCraftPart>,
    pub company_craft_processs: HashMap<CompanyCraftProcessId, CompanyCraftProcess>,
    pub company_craft_supply_items: HashMap<CompanyCraftSupplyItemId, CompanyCraftSupplyItem>,
    pub company_craft_draft_categorys:
        HashMap<CompanyCraftDraftCategoryId, CompanyCraftDraftCategory>,
    pub company_craft_types: HashMap<CompanyCraftTypeId, CompanyCraftType>,
    pub company_craft_drafts: HashMap<CompanyCraftDraftId, CompanyCraftDraft>,
    pub retainer_tasks: HashMap<RetainerTaskId, RetainerTask>,
    pub retainer_task_normals: HashMap<RetainerTaskNormalId, RetainerTaskNormal>,
    pub recipe_level_tables: HashMap<RecipeLevelTableId, RecipeLevelTable>,
    pub collectables_shop_items: HashMap<CollectablesShopItemId, Vec<CollectablesShopItem>>,
    pub collectables_shop_reward_scrips:
        HashMap<CollectablesShopRewardScripId, CollectablesShopRewardScrip>,
    pub craft_leves: HashMap<CraftLeveId, CraftLeve>,
}

impl HasId for Item {
    type Id = ItemId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for RecipeLevelTable {
    type Id = RecipeLevelTableId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CollectablesShopItem {
    type Id = CollectablesShopItemId;
    fn get_id(&self) -> Self::Id {
        CollectablesShopItemId(self.key_id.0)
    }
}
impl HasId for CollectablesShopRewardScrip {
    type Id = CollectablesShopRewardScripId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CraftLeve {
    type Id = CraftLeveId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for RetainerTask {
    type Id = RetainerTaskId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for RetainerTaskNormal {
    type Id = RetainerTaskNormalId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for Recipe {
    type Id = RecipeId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ClassJob {
    type Id = ClassJobId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ClassJobCategory {
    type Id = ClassJobCategoryId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for BaseParam {
    type Id = BaseParamId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for SpecialShop {
    type Id = SpecialShopId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for Leve {
    type Id = LeveId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for LeveRewardItem {
    type Id = LeveRewardItemId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for LeveRewardItemGroup {
    type Id = LeveRewardItemGroupId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ENpcBase {
    type Id = ENpcBaseId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ENpcResident {
    type Id = ENpcResidentId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for GilShop {
    type Id = GilShopId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for TopicSelect {
    type Id = TopicSelectId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for PreHandler {
    type Id = PreHandlerId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ItemSearchCategory {
    type Id = ItemSearchCategoryId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ItemUiCategory {
    type Id = ItemUiCategoryId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for ItemSortCategory {
    type Id = ItemSortCategoryId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftSequence {
    type Id = CompanyCraftSequenceId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftPart {
    type Id = CompanyCraftPartId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftProcess {
    type Id = CompanyCraftProcessId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftSupplyItem {
    type Id = CompanyCraftSupplyItemId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftDraftCategory {
    type Id = CompanyCraftDraftCategoryId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftType {
    type Id = CompanyCraftTypeId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}
impl HasId for CompanyCraftDraft {
    type Id = CompanyCraftDraftId;
    fn get_id(&self) -> Self::Id {
        self.key_id
    }
}

fn ok_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: Deserializer<'de>,
{
    Ok(T::deserialize(deserializer).unwrap_or_default())
}

#[cfg(test)]
mod tests {}
