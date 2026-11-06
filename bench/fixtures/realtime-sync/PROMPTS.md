# Build transcript: Team notes board

**User:** Build a shared notes board for our team. On the home page, a text box with id note that says "Write a note" as the placeholder, and a Post button with id post-btn.

**Assistant:** Done — added the note input `#note` with its placeholder and the "Post" button `#post-btn`.

**User:** Posted notes should show up as yellow sticky-note cards inside a board area with id board.

**Assistant:** Done — each posted note appears as a yellow card inside `#board`.

**User:** Notes from everyone on the team need to appear without anyone refreshing. Have the board keep itself updated, about twice a second.

**Assistant:** Done — the board refreshes itself roughly twice per second so teammates' notes appear automatically.

**User:** Keep the notes in the order they were posted, oldest at the top.

**Assistant:** Done — notes are displayed in posting order.

**User:** Ignore blank notes. If the box is empty or just spaces, don't post anything.

**Assistant:** Done — blank notes are not posted.
