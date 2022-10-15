use std::sync::Arc;

use maud::html;
use ultros_db::entity::{region, world};

use crate::{
    analyzer_service::ResaleStats,
    web::{
        oauth::AuthDiscordUser,
        templates::{components::{header::Header, gil::Gil, paginate::Paginate}, page::Page}, AnalyzerOptions,
    }, world_cache::{WorldCache, AnySelector},
};
use xiv_gen::ItemId;

pub(crate) struct AnalyzerPage {
    pub user: Option<AuthDiscordUser>,
    pub analyzer_results: Vec<ResaleStats>,
    pub world: world::Model,
    pub region: region::Model,
    pub options: AnalyzerOptions,
    pub world_cache: Arc<WorldCache>
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
        let options = serde_urlencoded::to_string(&options).unwrap_or_default();
        let paginate = Paginate::new(&self.analyzer_results, 25, page, options);
        let results = paginate.get_page();
        html! {
          ((Header {
            user: self.user.as_ref()
          }))
          div class="container" {
            div class="main-content" {
              div class="content-well" {
                span class="content-title" {
                  "resale analysis for " ((self.world.name)) " traveling within " ((self.region.name))
                }
                ((paginate))
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
                      a title="sort this table by profit" href="?sort=profit" { "profit" }
                    }
                    th {
                      a title="sort this table by return on investment" href="?sort=margin" { "roi" }
                    }
                    th {
                      "world"
                    }
                    th {
                      "datacenter"
                    }
                  }
                  @for result in results {
                    tr{
                      td{
                        a href={"/listings/" ((self.region.name)) "/" ((result.item_id))}{
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
