use crate::Cookies;
use leptos::*;
use leptos_router::*;

#[component]
pub fn Ad(#[prop(optional)] class: Option<&'static str>) -> impl IntoView {
    let ad_class = class.unwrap_or("h-64");
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let location = use_location();
    let pathname = location.pathname;
    move || {
        let _ = pathname(); // reading from the path to reload this component on page load
        (!hide_ads().unwrap_or_default()).then(move ||view!{
        <div class=ad_class>
            <div class="flex flex-col h-full">
                <span class="text-lg">"Ad"</span>
                <div class="flex-grow">
                    <script async src="https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client=ca-pub-8789160460804755"
                        crossorigin="anonymous"></script>
                    // <!-- Ultros-Ad-Main -->
                    <ins class="adsbygoogle"
                        style="display:block"
                        data-ad-client="ca-pub-8789160460804755"
                        data-ad-slot="1163555858"
                        data-ad-format="auto"
                        data-full-width-responsive="true"></ins>
                    <script>
                        (adsbygoogle = window.adsbygoogle || []).push({});
                    </script>
                </div>
                <span class="text-neutral-500 italic text-sm">"ads support the site. you may disable or enable them under "<A href="/settings">"Settings"</A></span>
            </div>
        </div>
    })
    }
}
