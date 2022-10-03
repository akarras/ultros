use maud::html;
use ultros_db::{
    entity::{region, world},
    price_optimizer::BestResellResults,
};

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{components::header::Header, page::Page},
};
use xiv_gen::ItemId;

pub(crate) struct AnalyzerPage {
    pub user: Option<AuthDiscordUser>,
    pub analyzer_results: Vec<BestResellResults>,
    pub world: world::Model,
    pub region: region::Model,
}

impl Page for AnalyzerPage {
    fn get_name<'a>(&self) -> &'a str {
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
                  "profit results for world " ((self.world.name))
                }
                table {
                  tr{
                    th {
                      "item"
                    }
                    th {
                      "profit"
                    }
                    th {
                      "margin"
                    }
                  }
                  @for result in &self.analyzer_results {
                    tr{
                      td{
                        a href={"/listings/" ((self.region.name)) "/" ((result.item_id))}{
                          img class="small-icon" src={"https://universalis-ffxiv.github.io/universalis-assets/icon2x/" (result.item_id) ".png"};
                          ((items.get(&ItemId(result.item_id)).map(|i| i.name.as_str()).unwrap_or_default()))
                        }
                      }
                      td {
                        ((result.profit))
                      }
                      td {
                        ((format!("{:.2}%", result.margin)))
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
