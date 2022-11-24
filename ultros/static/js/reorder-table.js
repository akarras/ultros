window.addEventListener('load', (e) => {
  for (let table of document.getElementsByClassName("reorder-table")) {
    let items = table.querySelectorAll("tbody > tr");
    for (let i of items) {
      console.log(i.firstChild.tagName);
      i.draggable = true;

      i.ondragstart = (ev) => {
        current = i;
        for (let it of items) {
          if (it != current) { it.classList.add("drop-hint"); }
        }
      };

      i.ondragenter = (ev) => {
        if (i != current) { i.classList.add("drag-active"); }
      };

      i.ondragleave = () => {
        i.classList.remove("drag-active");
      };

      i.ondragend = () => {
        for (let it of items) {
          it.classList.remove("drop-hint");
          it.classList.remove("drag-active");
        }

      };

      i.ondragover = (evt) => { evt.preventDefault(); };

      i.ondrop = (evt) => {
        evt.preventDefault();
        if (i != current) {
          let currentpos = 0, droppedpos = 0;
          for (let it = 0; it < items.length; it++) {
            if (current == items[it]) { currentpos = it; }
            if (i == items[it]) { droppedpos = it; }
          }
          if (currentpos < droppedpos) {
            i.parentNode.insertBefore(current, i.nextSibling);
          } else {
            i.parentNode.insertBefore(current, i);
          }
          var order = 0;
          let data = JSON.stringify(Array.from(i.parentNode.querySelectorAll("tr"), i => {
            let object = i.dataset;
            object.order = order;
            order++;
            return object;
          }));
          let postUrl = table.dataset.postUrl;
          let xhr = new XMLHttpRequest();
          xhr.open("POST", postUrl);
          xhr.setRequestHeader("Accept", "application/json");
          xhr.setRequestHeader("Content-Type", "application/json");
          xhr.onreadystatechange = function () {
            if (xhr.readyState === 4) {
              console.log(xhr.status);
              console.log(xhr.responseText);
            }
          };
          console.log(data);
          xhr.send(data);
        }

      };
    }

  }
})