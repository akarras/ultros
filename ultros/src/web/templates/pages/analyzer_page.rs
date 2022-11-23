use std::sync::Arc;

use maud::html;
use ultros_db::entity::{region, world};

use crate::{
    analyzer_service::ResaleStats,
    web::{
        oauth::AuthDiscordUser,
        templates::{
            components::{
                copy_text_button::CopyTextButton,
                gil::Gil,
                header::Header,
                item_icon::{IconSize, ItemIcon},
                paginate::Paginate,
                world_dropdown::WorldDropdown,
            },
            page::Page,
        },
        AnalyzerOptions, AnalyzerSort,
    },
    world_cache::{AnySelector, WorldCache},
};
use xiv_gen::ItemId;

pub(crate) struct AnalyzerPage {
    pub user: Option<AuthDiscordUser>,
    pub analyzer_results: Vec<ResaleStats>,
    pub world: Option<world::Model>,
    pub region: Option<region::Model>,
    pub options: AnalyzerOptions,
    pub world_cache: Arc<WorldCache>,
}

fn generate_temp_query<T>(options: &AnalyzerOptions, update: T) -> String
where
    T: FnOnce(&mut AnalyzerOptions),
{
    let mut value = options.clone();
    update(&mut value);
    serde_urlencoded::to_string(&value).unwrap_or_default()
}

impl Page for AnalyzerPage {
    fn get_name(&'_ self) -> &'_ str {
        "Analyzer"
    }

    fn draw_body(&self) -> maud::Markup {
        let items = &xiv_gen_db::decompress_data().items;
        let page = self.options.page.unwrap_or(1);
        let options_str = generate_temp_query(&self.options, |options| options.page = None);
        let paginate = Paginate::new(&self.analyzer_results, 75, page, options_str);
        let margin_query = generate_temp_query(&self.options, |options| {
            options.sort = Some(crate::web::AnalyzerSort::Margin)
        });
        let profit_query = generate_temp_query(&self.options, |options| {
            options.sort = Some(crate::web::AnalyzerSort::Profit)
        });

        let results = paginate.get_page();
        html! {
          ((Header {
            user: self.user.as_ref()
          }))
          div class="container" {
            div class="main-content" {
              div class="content-well" {
                ((paginate))
                form class="flex-row" {

                  div class="flex-column" {
                    label for="days" {
                      "sale within days:"
                    }
                    input id="days" name="days" type="number" value=((self.options.days.unwrap_or(100))) {}
                  }
                  div class="flex-column" {
                    label for="minimum_profit" {
                      "minimum profit:"
                    }
                    input id="minimum_profit" name="minimum_profit" type="number" value=((self.options.minimum_profit.unwrap_or(0))) {}
                  }
                  div class="flex-column" {
                    label for="world" {
                      "sell world:"
                    }
                    ((WorldDropdown { world_id: self.world.as_ref().map(|i| i.id), world_cache: &self.world_cache}))
                  }
                  input type="hidden" name="sort" id="sort" value=((self.options.sort.unwrap_or(AnalyzerSort::Margin))) {}
                  @if let Some(filter_world) = self.options.filter_world {
                    input type="hidden" name="filter_world" id="filter_world" value=((filter_world)) {}
                  }
                  @if let Some(filter_datacenter) = self.options.filter_datacenter {
                    input type="hidden" name="filter_datacenter" id="filter_datacenter" value=((filter_datacenter)) {}
                  }
                  div class="flex-column flex-end" {
                    input class="btn" type="submit" value="update" {}
                  }
                }
                @if let Some((world, region)) = self.world.as_ref().and_then(|w| self.region.as_ref().map(|r| (&w.name, &r.name))) {
                    span class="content-title" {
                    "resale analysis for " ((world)) " traveling within " ((region))
                    table {
                      tr{
                        th {
                          "hq"
                        }
                        th {
                          "item"
                        }
                        th {
                          "recently sold"
                        }
                        th {
                          "sale price"
                        }
                        th {
                          "-"
                        }
                        th {
                          "cheapest item"
                        }
                        th {
                          "="
                        }
                        th {
                          @if self.options.sort.map(|w| w == AnalyzerSort::Margin).unwrap_or_default() {
                            div class="tooltip" {
                              a href={"?" ((profit_query))} { "profit" }
                              span class="tooltip-text" {"sort this table by profit"}
                            }
                          } @else {
                            "profit"
                          }
                        }
                        th {
                          @if self.options.sort.map(|w| w == AnalyzerSort::Profit).unwrap_or(true) {
                            div class="tooltip" {
                              a href={"?" ((margin_query))} { "ROI" }
                              span class="tooltip-text" {"sort this table by return on investment"}
                            }
                          } @else {
                            "ROI"
                          }
                        }
                        th {
                          @if self.options.filter_world.is_some() {
                            div class="tooltip" {
                              a href={"?" ((generate_temp_query(&self.options, |o| o.filter_world = None)))} {
                                "world"
                              }
                              span class="tooltip-text" {
                                "clear world filter"
                              }
                            }
                          } @else {
                            "world"
                          }
                        }
                        th {
                          @if self.options.filter_datacenter.is_some() {
                            div class="tooltip" {
                              a href={"?" ((generate_temp_query(&self.options, |o| o.filter_datacenter = None)))} {
                                "datacenter"
                              }
                              span class="tooltip-text" {
                                "clear datacenter filter"
                              }
                            }
                          } @else {
                            "datacenter"
                          }
                        }
                      }
                      @for result in results {
                        tr {
                          td {
                            @if result.hq {
                              "✔️"
                            }
                          }
                          td{
                            div class="flex-row" {
                              @let item_name = items.get(&ItemId(result.item_id)).map(|i| i.name.as_str()).unwrap_or_default();
                              a href={"/listings/" ((region)) "/" ((result.item_id))}{
                                ((ItemIcon { item_id: result.item_id, icon_size: IconSize::Small }))
                                span class="width-limited-text" {((item_name))}
                              }
                              ((CopyTextButton { text: item_name }))
                            }
                          }
                          td {
                            ((result.sold_within))
                          }
                          td {
                            ((Gil(result.profit + result.cheapest)))
                          }
                          td {
                            "-"
                          }
                          td {
                            ((Gil(result.cheapest)))
                          }
                          td {
                            "="
                          }
                          td {
                            ((Gil(result.profit)))
                          }
                          td {
                            ((format!("{:.1}%", result.return_on_investment)))
                          }
                          @if let Ok(world) = self.world_cache.lookup_selector(&AnySelector::World(result.world_id)) {
                            td {
                              @if self.options.filter_world.is_none() {
                                div class="tooltip" {
                                  a href={"?" ((generate_temp_query(&self.options, |opt| { opt.filter_world = Some(result.world_id); opt.filter_datacenter = None; })))} {((world.get_name())) }
                                  span class="tooltip-text" {"only show best sales on " ((world.get_name()))}
                                }
                              } @else {
                                ((world.get_name()))
                              }
                            }
                            td {
                              // this will have one dc, but I just ran a loop because lazy
                              @if let Some(dcs) = self.world_cache.get_datacenters(&world) {
                                @for dc in dcs {
                                  div class="tooltip" {
                                    @if self.options.filter_datacenter.is_none() {
                                      a href={"?" ((generate_temp_query(&self.options, |opt| { opt.filter_datacenter = Some(dc.id); opt.filter_world = None; })))} {
                                        ((dc.name))
                                      }
                                      span class="tooltip-text" {"only show best sales in " ((dc.name))}
                                    } @else {
                                      ((dc.name))
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
                }
              }
            }
          }
        }
    }
}
