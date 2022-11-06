use std::sync::Arc;

use maud::html;
use ultros_db::entity::{active_listing, retainer, sale_history, unknown_final_fantasy_character};
use xiv_gen::ItemId;

use crate::{
    web::{
        home_world_cookie::HomeWorld,
        oauth::AuthDiscordUser,
        templates::{
            components::{
                copy_text_button::CopyTextButton,
                gil::Gil,
                header::Header,
                item_icon::{IconSize, ItemIcon},
            },
            page::Page,
        },
    },
    world_cache::{AnySelector, WorldCache},
};

pub(crate) struct ListingsPage {
    pub(crate) listings: Vec<(active_listing::Model, Option<retainer::Model>)>,
    pub(crate) sale_history: Vec<(
        sale_history::Model,
        Option<unknown_final_fantasy_character::Model>,
    )>,
    pub(crate) selected_world: String,
    pub(crate) home_world: Option<HomeWorld>,
    pub(crate) item_id: i32,
    pub(crate) item: &'static xiv_gen::Item,
    pub(crate) user: Option<AuthDiscordUser>,
    pub(crate) world_cache: Arc<WorldCache>,
}

impl Page for ListingsPage {
    fn get_name(&'_ self) -> &'_ str {
        xiv_gen_db::decompress_data()
            .items
            .get(&ItemId(self.item_id))
            .map(|i| i.name.as_str())
            .unwrap_or_default()
    }

    fn draw_body(&self) -> maud::Markup {
        let categories = &xiv_gen_db::decompress_data().item_ui_categorys;
        let mut low_quality_listings: Vec<_> =
            self.listings.iter().filter(|(l, _r)| !l.hq).collect();
        let mut high_quality_listings: Vec<_> =
            self.listings.iter().filter(|(l, _r)| l.hq).collect();
        low_quality_listings.sort_by_key(|(l, _)| l.price_per_unit);
        high_quality_listings.sort_by_key(|(l, _)| l.price_per_unit);
        let value = self.world_cache.lookup_value_by_name(&self.selected_world);
        let all = self.world_cache.get_all();
        let region = value
            .map(|w| {
                let region = self.world_cache.get_region(&w)?;
                let region = all.iter().find(|(r, _)| r.id == region.id)?;
                Some(region)
            })
            .ok()
            .flatten();

        html! {
          (Header {
            user: self.user.as_ref()
          })
          div class="container" {
            div class="flex-row flex-space" {
              div class="flex-column" {
                div class="search-result" {
                  ((ItemIcon{ item_id: self.item_id, icon_size: IconSize::Large }))
                  div class="search-result-details" {
                    span class="item-name" {
                      (&self.item.name)
                      ((CopyTextButton { text: &self.item.name }))
                    }
                    span class="item-type" {
                      (categories.get(&self.item.item_ui_category).map(|i| i.name.as_str()).unwrap_or_default())
                    }
                  }
                }
              }
              div class="content-nav nav" {
                @if let Some((region, datacenters)) = region {
                  div class="flex-column flex-end" {
                    div class="flex-row" {
                      @if region.name == self.selected_world {
                        a class="btn-secondary active" {
                          ((region.name))
                        }
                      } @else {
                        a class="btn-secondary" href={"/listings/" ((region.name)) "/" ((self.item_id))} {
                          ((region.name))
                        }
                      }
                      a class="btn-secondary" title="view on universalis" href={"https://universalis.app/market/" ((self.item_id))} {
                        "Universalis"
                      }
                      a class="btn-secondary" title="manually recheck universalis for updated data. usually unnecessary" href={"/listings/refresh/" ((self.selected_world)) "/" (self.item_id)} {
                        "Manual Refresh"
                      }
                    }
                    @for (datacenter, worlds) in datacenters {
                      div class="flex-row" {
                        @if datacenter.name == self.selected_world {
                          a class="btn-secondary active" {
                            ((datacenter.name))
                          }
                        } @else {
                          a class="btn-secondary" href={"/listings/" ((datacenter.name)) "/" ((self.item_id))} {
                            ((datacenter.name))
                          }
                        }
                        @for world in worlds {
                          a class={
                            "btn-secondary"
                            @if world.name == self.selected_world {
                              " active"
                            }
                            @if let Some(home_world) = &self.home_world {
                              @if world.id == home_world.home_world {
                                " homeworld"
                              }
                            }
                            } href={"/listings/" ((world.name)) "/" ((self.item_id))} {
                            ((world.name))
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
            div class="main-content flex-wrap" {
              @if !high_quality_listings.is_empty() {
                div class="content-well" {
                  span class="content-title" {
                    "high quality listings"
                  }
                  table {
                    tr {
                      th {
                        "price per unit"
                      }
                      th {
                        "qty."
                      }
                      th {
                        "total"
                      }
                      th {
                        "retainer name"
                      }
                      th {
                        "world"
                      }
                      th {
                        "datacenter"
                      }
                      th {
                        "first seen"
                      }
                    }
                    @for (listing, retainer) in high_quality_listings.iter().take(25) {
                      tr {
                        td {
                          ((Gil(listing.price_per_unit)))
                        }
                        td {
                          ((listing.quantity))
                        }
                        td {
                          ((Gil(listing.price_per_unit * listing.quantity)))
                        }
                        td {
                          @if let Some(retainer) = retainer {
                            a href={ "/retainers/listings/" ((retainer.id)) } { ((retainer.name)) }
                          }
                        }
                        @if let Ok(world) = self.world_cache.lookup_selector(&AnySelector::World(listing.world_id)) {
                          td {
                            ((world.get_name()))
                          }
                          td {
                            @for datacenter in self.world_cache.get_datacenters(&world).unwrap_or_default() {
                              ((datacenter.name))
                            }
                          }
                        }
                        td {
                          ((listing.timestamp.to_string()))
                        }

                      }
                    }
                  }
                }
              }
              @if !low_quality_listings.is_empty() {
                div class="content-well" {
                  span class="content-title" {
                    "low quality listings"
                  }
                  table {
                    tr {
                      th {
                        "price per unit"
                      }
                      th {
                        "qty."
                      }
                      th {
                        "total"
                      }
                      th {
                        "retainer name"
                      }
                      th {
                        "world"
                      }
                      th {
                        "datacenter"
                      }
                      th {
                        "first seen"
                      }
                    }
                    @for (listing, retainer) in low_quality_listings.iter().take(25) {
                      tr {
                        td {
                          ((Gil(listing.price_per_unit)))
                        }
                        td {
                          ((listing.quantity))
                        }
                        td {
                          ((Gil(listing.price_per_unit * listing.quantity)))
                        }
                        td {
                          @if let Some(retainer) = retainer {
                            a href={ "/retainers/listings/" ((retainer.id)) } { ((retainer.name)) }
                          }
                        }
                        @if let Ok(world) = self.world_cache.lookup_selector(&AnySelector::World(listing.world_id)) {
                          td {
                            ((world.get_name()))
                          }
                          td {
                            @for datacenter in self.world_cache.get_datacenters(&world).unwrap_or_default() {
                              ((datacenter.name))
                            }
                          }
                        }
                        td {
                          ((listing.timestamp.to_string()))
                        }
                      }
                    }
                  }
                }
              } @else if low_quality_listings.is_empty() && high_quality_listings.is_empty() {
                "no listings"
              }
              div class="content-well" {
                span class="content-title" {
                  "recent sales"
                }
                table {
                  tr {
                    
                    th {
                      "price per item"
                    }
                    th {
                      "qty."
                    }
                    th {
                      "total"
                    }
                    th {
                      "hq"
                    }
                    th {
                      "character name"
                    }
                    th {
                      "world"
                    }
                    th {
                      "datacenter"
                    }
                    th {
                      "date"
                    }
                  }
                  @for (sale, character) in &self.sale_history {
                    tr {
                      td {
                        ((Gil(sale.price_per_item)))
                      }
                      td {
                        ((sale.quantity))
                      }
                      td {
                        ((Gil(sale.price_per_item * sale.quantity)))
                      }
                      td {
                        @if sale.hq {
                          "✔️"
                        }
                      }
                      td {
                        @if let Some(character) = character {
                          ((character.name))
                        }
                      }
                      @if let Ok(world) = self.world_cache.lookup_selector(&AnySelector::World(sale.world_id)) {
                        td {
                          ((world.get_name()))
                        }
                        td {
                          @for datacenter in self.world_cache.get_datacenters(&world).unwrap_or_default() {
                            ((datacenter.name))
                          }
                        }
                      }
                      td {
                        ((sale.sold_date.to_string()))
                      }
                    }
                  }
                }
              }
            }
          }
        }
    }
}
