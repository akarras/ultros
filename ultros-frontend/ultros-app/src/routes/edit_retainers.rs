use leptos::*;

#[component]
pub fn EditRetainers(cx: Scope) -> impl IntoView {
    view! { cx, 
    <div class="container">
      <div class="content-nav">
        <a class="btn-secondary" href="/retainers/edit">
          <i class="fa-solid fa-magnifying-glass"></i>
          "Search Retainers"
        </a>
        <a class="btn-secondary active" href="/retainers/edit">
            <span class="fa fa-pen-to-square"></span>
            "Edit"
        </a>
        <a class="btn-secondary" href="/retainers/edit">
            <span class="fa fa-pencil"></span>
            "Listings"
        </a>
        <a class="btn-secondary" href="/retainers/edit">
            <span class="fa fa-exclamation"></span>
            "Undercuts"
        </a>
      </div>
      <div class="main-content">

      </div>
    </div>}
}
