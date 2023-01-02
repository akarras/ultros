use crate::search_box::*;
use leptos::*;

#[component]
pub fn MainNav(cx: Scope) -> impl IntoView {
    view! {
    cx,
    <div>
      <div class="gradient-outer">
          <div class="gradient"></div>
      </div>
      <header>
          <div class="header">
            <i><b>"ULTROS IS STILL UNDER ACTIVE DEVELOPMENT"</b></i>
            <a class="nav-item" href="/alerts">
              <i class="fa-solid fa-bell"></i>
              "Alerts"
            </a>
            <a href="/analyzer" class="nav-item">
              <i class="fa-solid fa-money-bill-trend-up"></i>
              "Analyzer"
            </a>
            <a href="/list" class="nav-item">
              <i class="fa-solid fa-list"></i>
              "Lists"
            </a>
            <a class="nav-item" href="/retainers">
              <i class="fa-solid fa-user-group"></i>
              "Retainers"
            </a>
            <SearchBox/>
            <a class="btn nav-item" href="/invitebot">
              "Invite Bot"
            </a>
          //   @if let Some(user) = self.user {
          //     <a class="btn nav-item" href="/logout">
          //       "Logout"
          //     </a>
          //     <a href="/profile">
          //       <img class="avatar" src=((user.avatar_url)) alt=((user.name))/>
          //     </a>
          //   } @else {
          //     <a class="btn nav-item" href="/login">
          //       "Login"
          //     </a>
          //   }
          </div>
      </header>
    </div>
    }
}
