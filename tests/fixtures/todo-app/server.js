const express = require('express');
const app = express();
const port = 8080;

app.use(express.json());

let todos = [
    { id: 1, text: "Finish Orchyn integration", completed: false },
];

app.get('/api/todos', (req, res) => {
    res.json(todos);
});

app.post('/api/todos', (req, res) => {
    const todo = { id: Date.now(), text: req.body.text, completed: false };
    todos.push(todo);
    res.status(201).json(todo);
});

app.delete('/api/todos/:id', (req, res) => {
    todos = todos.filter(t => t.id !== parseInt(req.params.id));
    res.status(204).send();
});

app.get('/', (req, res) => {
    res.send(`
        <!DOCTYPE html>
        <html>
        <head>
            <title>Multi-user Todo</title>
            <style>
                body { font-family: sans-serif; background: #121212; color: white; padding: 2rem; }
                input { padding: 0.5rem; border-radius: 4px; border: 1px solid #333; background: #222; color: white; }
                button { padding: 0.5rem 1rem; border-radius: 4px; border: none; background: #7C6BF8; color: white; cursor: pointer; }
                ul { list-style: none; padding: 0; }
                li { display: flex; justify-content: space-between; align-items: center; padding: 0.5rem; border-bottom: 1px solid #222; }
                .todo-text { flex: 1; }
            </style>
        </head>
        <body>
            <h1>Shared Team Todos</h1>
            <div id="app">
                <input type="text" id="todo-input" placeholder="What needs to be done?">
                <button id="add-btn">Add Todo</button>
                <ul id="todo-list"></ul>
            </div>
            <script>
                const input = document.getElementById('todo-input');
                const btn = document.getElementById('add-btn');
                const list = document.getElementById('todo-list');

                async function fetchTodos() {
                    const res = await fetch('/api/todos');
                    const data = await res.json();
                    list.innerHTML = '';
                    data.forEach(todo => {
                        const li = document.createElement('li');
                        li.innerHTML = \`
                            <span class="todo-text">\${todo.text}</span>
                            <button onclick="deleteTodo(\${todo.id})">Delete</button>
                        \`;
                        list.appendChild(li);
                    });
                }

                async function addTodo() {
                    if (!input.value) return;
                    await fetch('/api/todos', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ text: input.value })
                    });
                    input.value = '';
                    fetchTodos();
                }

                async function deleteTodo(id) {
                    await fetch('/api/todos/' + id, { method: 'DELETE' });
                    fetchTodos();
                }

                btn.onclick = addTodo;
                fetchTodos();
                
                // Poll for updates to simulate multi-user sync
                setInterval(fetchTodos, 2000);
            </script>
        </body>
        </html>
    `);
});

app.listen(port, () => {
    console.log(`Todo app listening at http://localhost:${port}`);
});
