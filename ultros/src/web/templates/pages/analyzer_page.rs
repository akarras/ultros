use std::sync::Arc;

use maud::html;
use ultros_db::entity::{region, world};

use crate::{
    analyzer_service::ResaleStats,
    web::{
        oauth::AuthDiscordUser,
        templates::{
            components::{gil::Gil, header::Header, paginate::Paginate, world_dropdown::WorldDropdown},
            page::Page,
        },
        AnalyzerOptions, AnalyzerSort, home_world_cookie::HomeWorld,
    },
    world_cache::{AnySelector, WorldCache},
};
use xiv_gen::ItemId;

pub(crate) struct AnalyzerPage {
    pub user: Option<AuthDiscordUser>,
    pub analyzer_results: Vec<ResaleStats>,
    pub world: Option<world::Model>,
    pub home_world: Option<HomeWorld>,
    pub region: Option<region::Model>,
    pub options: AnalyzerOptions,
    pub world_cache: Arc<WorldCache>,
}

impl Page for AnalyzerPage {
    fn get_name<'a>(&'a self) -> &'a str {
        "Analyzer"
    }

    fn draw_body(&self) -> maud::Markup {
        let items = &xiv_gen_db::decompress_data().items;
        let page = self.options.page.unwrap_or_default();
        let mut options = self.options.clone();
        options.page = None;
        let options_str = serde_urlencoded::to_string(&options).unwrap_or_default();
        let paginate = Paginate::new(&self.analyzer_results, 75, page, options_str);
        options = self.options.clone();
        options.sort = Some(crate::web::AnalyzerSort::Margin);
        let margin_query = serde_urlencoded::to_string(&options).unwrap_or_default();
        options.sort = Some(crate::web::AnalyzerSort::Profit);
        let profit_query = serde_urlencoded::to_string(&options).unwrap_or_default();
        let results = paginate.get_page();
        html! {
          ((Header {
            user: self.user.as_ref()
          }))
          div class="container" {
            div class="main-content" {
              div class="content-well" {
                ((paginate))
                form {
                  label for="days" {
                    "sale within days:"
                  }
                  input id="days" name="days" type="number" value=((self.options.days.unwrap_or(100))) {}
                  label for="minimum_profit" {
                    "minimum profit:"
                  }
                  input id="minimum_profit" name="minimum_profit" type="number" value=((self.options.minimum_profit.unwrap_or(0))) {}
                  label for="world" {
                    "sell world:"
                  }
                  ((WorldDropdown { world_id: self.world.as_ref().map(|i| i.id), world_cache: &self.world_cache}))
                  input type="hidden" name="sort" id="sort" value=((self.options.sort.unwrap_or(AnalyzerSort::Margin))) {}
                  input class="btn" type="submit" value="update" {}
                }
                @if let Some((world, region)) = self.world.as_ref().map(|w| self.region.as_ref().map(|r| (&w.name, &r.name))).flatten() {
                    span class="content-title" {
                    "resale analysis for " ((world)) " traveling within " ((region))
                    table {
                      tr{
                        th {
                          "item"
                        }
                        th {
                          "hq"
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
                          a title="sort this table by profit" href={"?" ((profit_query))} { "profit" }
                        }
                        th {
                          a title="sort this table by return on investment" href={"?" ((margin_query))} { "roi" }
                        }
                        th title="world this item is cheapest on" {
                          "world"
                        }
                        th title="datacenter this item is cheapest on" {
                          "datacenter"
                        }
                      }
                      @for result in results {
                        tr {
                          td{
                            a href={"/listings/" ((region)) "/" ((result.item_id))}{
                              img class="small-icon" src={"https://universalis-ffxiv.github.io/universalis-assets/icon2x/" (result.item_id) ".png"};
                              ((items.get(&ItemId(result.item_id)).map(|i| i.name.as_str()).unwrap_or_default()))
                            }
                          }
                          th {
                            @if result.hq {
                              "✔️"
                            }
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
                              ((world.get_name()))
                            }
                            td {
                              // this will have one dc, but I just ran a loop because lazy
                              @if let Some(dc) = self.world_cache.get_datacenters(&world) {
                                @for dcs in dc {
                                  ((dcs.name))
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
