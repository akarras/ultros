function do_search(query) {
  // todo debounce this
  var xhttp = new XMLHttpRequest();
  xhttp.onload = function() {

    document.getElementById("search-results").innerHTML = xhttp.responseText;
  }
  xhttp.open("GET", "/items/" + encodeURIComponent(query), true);
  xhttp.send();
}

window.onload = function() {
  var search_box = document.getElementById("search-box");
  var search_results = document.getElementById("search-results");
  var wants_close = false;
  search_box.addEventListener('input', (event) => {
      console.log("text changed", event.target.value);
      do_search(event.target.value);
  });
  search_box.addEventListener('focus', (event) => {
    search_box.classList.add('active');
  });
  search_box.addEventListener('focusout', (event) => {
    wants_close = true;
    setTimeout(() => {
      if (wants_close) {
        search_box.classList.remove('active');
      }
    }, 100);
  });
  search_results.addEventListener('focusin', (event) => {
    console.log("focused in on results");
    wants_close = false;
  });
  search_results.addEventListener('focusout', (event) => {
    console.log('focus out results');
    setTimeout(() => {
      if (wants_close) {
        search_box.classList.remove('active');
      }
    }, 100);
    wants_close = true;
  });
  search_results.addEventListener('click', (event) => {
    wants_close = false;
  });
  search_results.addEventListener('hover', (event) => {
    wants_close = false;
  });
  search_results.addEventListener('mouseleave', (event) => {
    if (document.activeElement != search_box) {
      search_box.classList.remove('active');
    }
  })
}