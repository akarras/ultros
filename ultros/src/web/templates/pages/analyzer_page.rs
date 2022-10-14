use maud::html;
use ultros_db::entity::{region, world};

use crate::{
    analyzer_service::ResaleStats,
    web::{
        oauth::AuthDiscordUser,
        templates::{components::header::Header, page::Page},
    },
};
use xiv_gen::ItemId;

pub(crate) struct AnalyzerPage {
    pub user: Option<AuthDiscordUser>,
    pub analyzer_results: Vec<ResaleStats>,
    pub world: world::Model,
    pub region: region::Model,
}

impl Page for AnalyzerPage {
    fn get_name<'a>(&'a self) -> &'a str {
        "Analyzer"
    }

    fn draw_body(&self) -> maud::Markup {
        let items = &xiv_gen_db::decompress_data().items;
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
                      a title="sort thsi table by profit" href="?sort=profit" { "profit" }
                    }
                    th {
                      a title="sort this table by margin" href="?sort=margin" { "margin" }
                    }
                    th {
                      "world"
                    }
                    th {
                      "datacenter"
                    }
                  }
                  @for result in self.analyzer_results.iter().take(100) {
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
                        ((result.profit + result.cheapest))
                      }
                      td {
                        "-"
                      }
                      td {
                        ((result.cheapest))
                      }
                      td {
                        "="
                      }
                      td {
                        ((result.profit))
                      }
                      td {
                        ((format!("{:.2}%", (result.profit + result.cheapest) as f32 / result.cheapest as f32 * 100.0)))
                      }
                      td {
                        "n/a"
                      }
                      td {
                        "n/a"
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
