use std::num::ParseIntError;

use leptos::*;
use thiserror::Error;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use crate::api::bulk_add_item_to_list;

#[derive(Error, Debug)]
pub enum ParseListError {
    #[error("Error parsing integer {0}")]
    ParseIntError(#[from] ParseIntError),
    #[error("Item ID not found for item with string: {0}")]
    ItemIdNotFound(String),
}

fn lookup_item_by_name(name: &str) -> Result<ItemId, ParseListError> {
    let items = &xiv_gen_db::decompress_data().items;
    items
        .iter()
        .find(|(_, item)| item.name == name.trim())
        .map(|(i, _)| i.clone())
        .ok_or_else(|| ParseListError::ItemIdNotFound(name.to_string()))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MakePlaceItemData {
    pub item_id: i32,
    pub quantity: i32,
}

/// Parse a MakePlace formatted list (or really any list following the format of name: quantity)
fn parse_list(list: &str) -> Result<Vec<MakePlaceItemData>, ParseListError> {
    // Lists come with duplicated data in sections.
    // For our purposes, we want to read everything but the furniture + dye section.
    // Once we get to the dye section, we will need to add dye to the name to get the lookup to work correctly.
    list.lines()
        // Try to split based on :. If we don't have a :, we don't have an item, and can ignore it
        .peekable()
        // remove furniture + dye
        .filter(|line| !line.contains("("))
        .flat_map(|line| line.split_once(":"))
        .map(|(item_name, quantity)| {
            println!("{item_name}: {quantity}");
            let quantity = quantity.trim().parse::<i32>()?;
            // TODO: There are some dyes that have unmarketable variants such as Pure White Dye -> General Purpose Pure White Dye
            // We should be able to automatically find the general-purpose version in the future
            let item_id = lookup_item_by_name(item_name)
                .or_else(|_| lookup_item_by_name(&format!("{} Dye", item_name.trim())))?
                .0;
            Ok(MakePlaceItemData { item_id, quantity })
        })
        .collect()
}

#[component]
pub fn MakePlaceImporter(list_id: Signal<i32>) -> impl IntoView {
    let (list, set_list) = create_signal("".to_string());
    let add_items_to_list = create_action(move |list_items: &Vec<MakePlaceItemData>| {
        let list_items = list_items.clone();
        async move {
            let items = list_items
                .into_iter()
                .map(|data| ListItem {
                    item_id: data.item_id,
                    quantity: Some(data.quantity),
                    ..Default::default()
                })
                .collect();
            bulk_add_item_to_list(list_id(), items).await
        }
    });
    view! {
        <div class="flex-column">
            <label>"Copy+Paste a list with a bunch of items in it formatted as Item1: Quantity. Make place users can paste their furniture+dye lists here."</label>
            <textarea on:input=move |input| set_list(event_target_value(&input))></textarea>
            <button on:click=move |_| {
                if let Ok(list) = parse_list(&list()) {
                    add_items_to_list.dispatch(list);
                }
            } class="btn">"Bulk add"</button>
        </div>
    }
}

#[cfg(test)]
mod test {
    use super::{parse_list, MakePlaceItemData};

    #[test]
    fn test_parse() {
        let list = parse_list(TEST_DATA).unwrap();
        assert!(list.contains(&MakePlaceItemData {
            item_id: 33258,
            quantity: 2
        }));
        assert!(list.contains(&MakePlaceItemData {
            item_id: 30116,
            quantity: 7
        }))
    }

    const TEST_DATA: &'static str = r#"     Furniture     
    =====================
    Eastern Indoor Pond: 2
    Out on a Limb Machine: 1
    Steel Locker: 1
    Fat Cat Bank: 1
    Waterfall Partition: 3
    Stuffed Android Units: 1
    Wine Barrel: 2
    Wine Rack: 1
    Manor Bookshelf: 1
    White Rectangular Partition: 29
    Luminous Wooden Loft: 18
    Wooden Handrail: 13
    Manor Couch: 13
    Fat Cat Sofa: 1
    Royal Plotting Table: 2
    Hannish Rug: 2
    Wooden Steps: 7
    Back Bar: 6
    Chirurgeon's Curtain: 1
    Armor Hanger: 1
    Hingan Bed: 1
    Kugane Phasmascape: 1
    Manor Highback Chair: 8
    White Partition: 12
    Windowed Partition: 1
    White Half Partition: 2
    Stage Panel: 4
    Nameday Cake: 1
    Stage Curtain: 2
    Troupe Stage: 1
    Mender Permit A-2: 1
    Simple Sink: 3
    Wooden Blinds: 3
    Dish Rack: 1
    Riviera Stool: 28
    Glade Round Table: 7
    Mounted Bookshelf: 4
    Crystarium Bench: 8
    Scholasticate Table: 4
    Cutting Board: 1
    Oriental Bathtub: 1
    Amigo Cactus Floor Lamp: 1
    Wood Slat Partition: 8
    Verdant Partition: 10
    Guildleve Counter: 1
    Bag of Booty: 1
    Tatami Loft: 8
    Deluxe Manor Fireplace: 1
    Enigma Partition: 3
    Blades of Innocence: 1
    Royal Partition: 1
    Manor Flower Stand: 1
    Message Book Stand: 1
    Oasis Wardrobe: 1
    Factory Automatic Door: 1
    Lily Wall Lamp: 2
    Paissa Floor Lamp: 4
    Manor Timpani: 1
    Manor Cello: 1
    Potted Maguey: 1
    Steward Permit: 3
    Manor Marching Horn: 1
    Ivy Curtain: 4
    Oldrose Wall Planter: 3
    Apparel Showcase: 1
    Eastern Canopy Bed: 1
    Rose Trellis: 2
    Summoning Bell: 1
    Crystal Bell: 1
    Gaol Partition: 2
    Manor Dressing Table: 1
    Ruby Weapon Bust: 1
    Violin Showcase: 1
    Red Carpet: 1
    Manor Harp: 1
    Philosopher's Stone Table: 1
    Wall Planter: 1
    Flagstone Steps: 8
    Flagstone Loft: 3
    Swag Valance: 3
    Beyond Gloam's Veil: 1
    Log Pillar: 1
    Unmoving Maneki Moogle: 1
    Glade Cupboard: 1
    Glade Sideboard: 1
    Titania Shadow Box: 1
    Riviera Wall Shelf: 1
    Baked Goods Showcase: 1
    Pub Signboard: 1
    Manor Harpsichord: 1
    Manor Music Stool: 1
    Sharlayan Rug: 1
    Riviera Flower Vase: 1
    Odder Otter Wall Lantern: 4
    Pudding Settee: 1
    Armament Showcase: 1
    Carbuncle Bathtub: 1
    Bathroom Floor Tiles: 3
    Hanging Planter Branch: 1
    Pine Bonsai: 2
    Simple Curtain: 2
    Alpine Cabinet: 1
    Kotatsu Table: 1
    Dance Pole: 1
    Eulmoran Divan: 1
    Glade Bathtub: 1
    Smithing Bench: 1
    Red Brick Counter: 4
    Drinking Apkallu: 1
    Hanging Planter Shelf: 1
    Stuffed Cait Sith: 1
    Wall-mounted Wings: 1
    Pile of Tomes: 1
    Trick Bookshelf Partition: 2
    Paper Partition: 1
    Gaol Partition Door: 1
    
            Dyes        
    =====================
    Blood Red: 5
    Dalamud Red: 14
    Dark Red: 22
    Pure White: 1
    Ruby Red: 7
    Russet Brown: 20
    Rust Red: 72
    Wine Red: 76
    
    Furniture (With Dye)
    =====================
    Alpine Cabinet (Rust Red): 1
    Amigo Cactus Floor Lamp: 1
    Apparel Showcase (Wine Red): 1
    Armament Showcase (Wine Red): 1
    Armor Hanger: 1
    Back Bar (Rust Red): 6
    Bag of Booty: 1
    Baked Goods Showcase: 1
    Bathroom Floor Tiles (Rust Red): 1
    Bathroom Floor Tiles (Wine Red): 2
    Beyond Gloam's Veil: 1
    Blades of Innocence: 1
    Carbuncle Bathtub (Rust Red): 1
    Chirurgeon's Curtain (Rust Red): 1
    Crystal Bell: 1
    Crystarium Bench (Wine Red): 8
    Cutting Board: 1
    Dance Pole: 1
    Deluxe Manor Fireplace (Rust Red): 1
    Dish Rack: 1
    Drinking Apkallu: 1
    Eastern Canopy Bed: 1
    Eastern Indoor Pond: 2
    Enigma Partition (Blood Red): 3
    Eulmoran Divan (Ruby Red): 1
    Factory Automatic Door: 1
    Fat Cat Bank: 1
    Fat Cat Sofa: 1
    Flagstone Loft (Wine Red): 3
    Flagstone Steps (Wine Red): 8
    Gaol Partition: 2
    Gaol Partition Door: 1
    Glade Bathtub: 1
    Glade Cupboard (Blood Red): 1
    Glade Round Table (Rust Red): 7
    Glade Sideboard (Blood Red): 1
    Guildleve Counter (Rust Red): 1
    Hanging Planter Branch: 1
    Hanging Planter Shelf: 1
    Hannish Rug: 2
    Hingan Bed: 1
    Ivy Curtain (Ruby Red): 4
    Kotatsu Table: 1
    Kugane Phasmascape: 1
    Lily Wall Lamp: 2
    Log Pillar (Wine Red): 1
    Luminous Wooden Loft (Russet Brown): 10
    Luminous Wooden Loft (Rust Red): 8
    Manor Bookshelf (Dalamud Red): 1
    Manor Cello: 1
    Manor Couch (Rust Red): 8
    Manor Couch (Wine Red): 5
    Manor Dressing Table (Wine Red): 1
    Manor Flower Stand: 1
    Manor Harp: 1
    Manor Harpsichord (Wine Red): 1
    Manor Highback Chair: 8
    Manor Marching Horn: 1
    Manor Music Stool: 1
    Manor Timpani: 1
    Mender Permit A-2: 1
    Message Book Stand: 1
    Mounted Bookshelf: 4
    Nameday Cake: 1
    Oasis Wardrobe: 1
    Odder Otter Wall Lantern: 4
    Oldrose Wall Planter: 3
    Oriental Bathtub: 1
    Out on a Limb Machine: 1
    Paissa Floor Lamp: 4
    Paper Partition (Wine Red): 1
    Philosopher's Stone Table: 1
    Pile of Tomes: 1
    Pine Bonsai (Ruby Red): 2
    Potted Maguey: 1
    Pub Signboard: 1
    Pudding Settee (Rust Red): 1
    Red Brick Counter: 4
    Red Carpet: 1
    Riviera Flower Vase: 1
    Riviera Stool (Rust Red): 28
    Riviera Wall Shelf: 1
    Rose Trellis: 2
    Royal Partition: 1
    Royal Plotting Table: 2
    Ruby Weapon Bust: 1
    Scholasticate Table (Dark Red): 4
    Sharlayan Rug: 1
    Simple Curtain (Rust Red): 2
    Simple Sink: 2
    Simple Sink (Rust Red): 1
    Smithing Bench: 1
    Stage Curtain: 2
    Stage Panel (Dark Red): 2
    Stage Panel (Wine Red): 2
    Steel Locker: 1
    Steward Permit: 3
    Stuffed Android Units: 1
    Stuffed Cait Sith: 1
    Summoning Bell (Rust Red): 1
    Swag Valance: 3
    Tatami Loft (Dark Red): 8
    Titania Shadow Box: 1
    Trick Bookshelf Partition: 2
    Troupe Stage (Dark Red): 1
    Unmoving Maneki Moogle: 1
    Verdant Partition: 10
    Violin Showcase (Wine Red): 1
    Wall Planter: 1
    Wall-mounted Wings (Pure White): 1
    Waterfall Partition: 3
    White Half Partition (Wine Red): 2
    White Partition (Wine Red): 12
    White Rectangular Partition (Dark Red): 4
    White Rectangular Partition (Wine Red): 25
    Windowed Partition (Wine Red): 1
    Wine Barrel: 2
    Wine Rack: 1
    Wood Slat Partition (Russet Brown): 8
    Wooden Blinds (Dark Red): 3
    Wooden Handrail (Dalamud Red): 13
    Wooden Steps (Russet Brown): 2
    Wooden Steps (Rust Red): 4
    Wooden Steps (Wine Red): 1
    "#;
}
