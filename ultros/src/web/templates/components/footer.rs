use maud::{Render, html};



pub(crate) struct Footer {
    
}

impl Render for Footer {
    fn render(&self) -> maud::Markup {
        html!{
            footer {
                div class="flex-row flex-space" {
                    div class="flex-column" {
                        span {"Made using " a href="https://universalis.app/" { "universalis" } "' API."  "Please contribute to Universalis to help this site."}
                        span {""}
                        span {"FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved."}
                    }
                }
            }
        }
    }
}


