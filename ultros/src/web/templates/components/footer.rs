use maud::{html, Render};

pub(crate) struct Footer;

impl Render for Footer {
    fn render(&self) -> maud::Markup {
        html! {
            footer class="flex-column flex-space flex-center" {
                div class="flex-row column-pad" {a href="https://discord.gg/pgdq9nGUP2" {"Discord"} "|" a href="https://github.com/akarras/ultros" {"GitHub"} "|" a href="https://leekspin.com" {"Patreon"}}
                span {"Made using " a href="https://universalis.app/" { "universalis" } "' API."  "Please contribute to Universalis to help this site stay up to date."}
                span {""}
                span {"FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."}
            }
        }
    }
}
