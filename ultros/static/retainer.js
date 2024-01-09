window.addEventListener('load', () => {
  console.log("retainer search updated");
    let retainer_name = document.getElementById("retainer-name");
    let search_button = document.getElementById("retainer-button");
    retainer_name.addEventListener('input', (e) => {
      search_button.href = "/retainers/add?search=" + encodeURIComponent(e.target.value);
    });
    retainer_name.addEventListener('keydown', (e) => {
      if (!e) { var e = window.event; }
      if (e.key == "Enter") {
        e.preventDefault();
        window.location.href = search_button.href;
      }

    })
});