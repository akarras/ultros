use std::sync::Arc;

use maud::{html, Render};
use ultros_db::world_cache::{AnyResult, AnySelector, WorldCache};

pub(crate) struct WorldDropdown<'a> {
    pub(crate) world_id: Option<i32>,
    pub(crate) world_cache: &'a Arc<WorldCache>,
}

impl<'a> Render for WorldDropdown<'a> {
    fn render(&self) -> maud::Markup {
        let all = self.world_cache.get_all();
        let home_world = self.world_id.unwrap_or_default();
        let world = self
            .world_cache
            .lookup_selector(&AnySelector::World(home_world))
            .map(|w| match w {
                AnyResult::World(world) => Some(world),
                _ => None,
            })
            .ok()
            .flatten();
        html! {
            select name="world" id="worlddropdown" {
                @if let Some(world) = world {
                    option value=((world.id)) active {
                        ((world.name))
                    }
                }
                @for (region, datacenters) in all {
                    optgroup label=((region.name)) {
                        @for (datacenter, worlds) in datacenters {
                            optgroup label=((datacenter.name)) {
                                @for world in worlds {
                                    option value=((world.id)) {
                                        ((world.name))
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
