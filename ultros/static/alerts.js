
function notifyMe() {
    if (!("Notification" in window)) {
      // Check if the browser supports notifications
      alert("This browser does not support desktop notification");
    } else if (Notification.permission === "granted") {
      // Check whether notification permissions have already been granted;
      // if so, create a notification
      const notification = new Notification("Hi there!");
      // …
    } else if (Notification.permission !== "denied") {
      // We need to ask the user for permission
      Notification.requestPermission().then((permission) => {
        // If the user accepts, let's create a notification
        if (permission === "granted") {
          const notification = new Notification("Hi there!");
          // …
        }
      });
    }
}

window.addEventListener('load', (event) => {
    console.log('starting websocket');
    const s = new WebSocket(((window.location.protocol === "https:") ? "wss://" : "ws://") + window.location.host + "/alerts/websocket");
    s.onerror = (e) => {
        console.error("websocket error:", e);
    };
    s.onclose = (c) => {
        console.error('closed ', c);
    };
    s.onopen = (o) => {
        var subscription = JSON.stringify({
            "Undercuts": {
                'margin': 100
            }
        });
        console.log(s, subscription);
        s.send(subscription);
    };
    s.onmessage = (e) => {
        let websocket_message = JSON.parse(e.data);
        console.log("websocket message", websocket_message);
        if (websocket_message.RetainerUndercut) {
            // pub(crate) id: i32,
            // pub(crate) name: String,
            // pub(crate) undercut_amount: i32,
            let {item_id, item_name, undercut_retainers} = websocket_message;
            var retainer_strs = [];
            for (let retainer in undercut_retainers) {
                retainer_strs.push("<li><a href=\"/retainers/\"" + retainer.id + ">" + retainer.name + "</a></li>");
            }
            var frame = document.getElementById("alert-frame");
            frame.innerHTML += "<div><h3>"+ item_name +"</h3><ul>"+retainer_strs.join("")+"</ul></div>";
            if (Notification.permission === "granted") {
                // Check whether notification permissions have already been granted;
                // if so, create a notification
                const notification = new Notification(retainer_strs + " have been undercut!");
            }
        }
    };

})

