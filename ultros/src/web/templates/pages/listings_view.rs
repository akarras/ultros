use std::sync::Arc;

use maud::html;
use ultros_db::entity::{active_listing, retainer};
use xiv_gen::ItemId;

use crate::{
    web::{
        oauth::AuthDiscordUser,
        templates::{components::header::Header, page::Page},
    },
    world_cache::{AnySelector, WorldCache},
};

pub(crate) struct ListingsPage {
    pub(crate) listings: Vec<(active_listing::Model, Option<retainer::Model>)>,
    pub(crate) selected_world: String,
    pub(crate) worlds: Vec<String>,
    pub(crate) item_id: i32,
    pub(crate) item: &'static xiv_gen::Item,
    pub(crate) user: Option<AuthDiscordUser>,
    pub(crate) world_cache: Arc<WorldCache>,
}

impl Page for ListingsPage {
    fn get_name<'b>(&'b self) -> &'b str {
        xiv_gen_db::decompress_data()
            .items
            .get(&ItemId(self.item_id))
            .map(|i| i.name.as_str())
            .unwrap_or_default()
    }

    fn draw_body(&self) -> maud::Markup {
        let categories = &xiv_gen_db::decompress_data().item_ui_categorys;
        let mut low_quality_listings: Vec<_> =
            self.listings.iter().filter(|(l, r)| !l.hq).collect();
        let mut high_quality_listings: Vec<_> =
            self.listings.iter().filter(|(l, r)| l.hq).collect();
        low_quality_listings.sort_by_key(|(l, _)| l.price_per_unit);
        high_quality_listings.sort_by_key(|(l, _)| l.price_per_unit);
        html! {
          (Header {
            user: self.user.as_ref()
          })
          div class="container" {
            div class="search-result" {
              img src={"https://universalis-ffxiv.github.io/universalis-assets/icon2x/" (self.item_id) ".png"};
              div class="search-result-details" {
                span class="item-name" {
                  (&self.item.name)
                }
                span class="item-type" {
                  (categories.get(&self.item.item_ui_category).map(|i| i.name.as_str()).unwrap_or_default())
                }
              }
            }
            div class="content-nav nav" {
              @for world_name in &self.worlds {
                @if world_name == &self.selected_world {
                  a class="btn-secondary active" {
                    ((world_name))
                  }
                } @else {
                  a class="btn-secondary" href={"/listings/" ((world_name)) "/" ((self.item_id))} {
                    ((world_name))
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
                        "quantity"
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
                          ((listing.price_per_unit))
                        }
                        td {
                          ((listing.quantity))
                        }
                        td {
                          ((listing.price_per_unit * listing.quantity))
                        }
                        td {
                          @if let Some(retainer) = retainer {
                            a href={ "/retainers/listings/" ((retainer.id)) } { ((retainer.name)) }
                          }
                        }
                        @if let Some(world) = self.world_cache.lookup_selector(&AnySelector::World(listing.world_id)) {
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
                        "quantity"
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
                          ((listing.price_per_unit))
                        }
                        td {
                          ((listing.quantity))
                        }
                        td {
                          ((listing.price_per_unit * listing.quantity))
                        }
                        td {
                          @if let Some(retainer) = retainer {
                            a href={ "/retainers/listings/" ((retainer.id)) } { ((retainer.name)) }
                          }
                        }
                        @if let Some(world) = self.world_cache.lookup_selector(&AnySelector::World(listing.world_id)) {
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
            }
          }
        }
    }
}
