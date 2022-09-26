window.addEventListener('load', () => {
  console.log("retainer search updated");
    let retainer_name = document.getElementById("retainer-name");
    let search_button = document.getElementById("retainer-button");
    retainer_name.oninput = function(event) {
      search_button.href = "/retainers/add?search=" + encodeURIComponent(event.target.value);
    }
});