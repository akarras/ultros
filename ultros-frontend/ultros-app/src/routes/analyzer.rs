use leptos::*;

#[component]
pub fn Analyzer(cx: Scope) -> impl IntoView {
    view! {
        cx,
        <div class="container">
            <div class="main-content flex flex-center">
                <div>
                    <span class="content-title">"Analyzer"</span>
                    <div>
                        <span>"The analyzer helps find items that are cheaper on other worlds that sell for more on your world."</span>
                        <span>"Adjust parameters to try and find items that sell well"</span>
                    </div>
                </div>
            </div>
        </div>
    }
}
