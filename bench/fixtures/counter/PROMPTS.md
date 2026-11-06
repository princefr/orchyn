# Build transcript: Shared counter

**User:** Build me a shared counter. One page at the home URL showing "Count: 0" in an element with id count. Everyone who opens it should see the same number.

**Assistant:** Done — the home page shows a shared "Count: N" in `#count`, with the count kept on the server.

**User:** Keep the number on the server, not in the browser, so it's the same for everybody. When the page loads it should show whatever the server currently has.

**Assistant:** Done — the page fetches the current value from the server on load.

**User:** Add a button labelled "Increment" with id inc. Each click adds one and then shows whatever number the server sends back.

**Assistant:** Done — `#inc` sends one increment per click and displays the server's returned value.

**User:** Important: the page should never change the number by itself. No timers, no auto-counting. Only a click on Increment should move it.

**Assistant:** Done — the count only changes when Increment is clicked; nothing updates it automatically.

**User:** Make sure one click always adds exactly one, even if I click fast.

**Assistant:** Done — each click sends a single increment request, so every click adds exactly one.
