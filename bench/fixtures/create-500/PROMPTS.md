# Build transcript: Todo list

**User:** I need a todo list page. A text box with the id title, and it should say "What needs doing?" as the placeholder. Next to it an Add button with id add-btn.

**Assistant:** Done — added the todo input `#title` with the placeholder and the "Add" button `#add-btn`.

**User:** When I click Add, save whatever I typed as a new todo on the server, using /api/todos.

**Assistant:** Done — Add posts the typed text to `/api/todos`.

**User:** Below the box, show a list with id list with every todo the server has. They should show up in the order I added them.

**Assistant:** Done — `#list` displays all todos from the server in the order they were added.

**User:** If I click Add with the box empty or only spaces, do nothing at all. Don't send anything to the server.

**Assistant:** Done — blank input is ignored and no request is sent.

**User:** Clear the text box after every Add so I can type the next one right away.

**Assistant:** Done — the input is cleared after each Add.
