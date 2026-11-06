const path = require('path');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

const todos = [];
let nextId = 1;

const app = express();
app.use(express.json());

app.get('/api/todos', (req, res) => {
  res.json(todos);
});

app.post('/api/todos', (req, res) => {
  if (BUG) {
    return res.status(500).json({ error: 'database unavailable' });
  }
  const title = String((req.body && req.body.title) || '').trim();
  if (!title) {
    return res.status(400).json({ error: 'title required' });
  }
  const todo = { id: nextId++, title, done: false };
  todos.push(todo);
  res.status(201).json(todo);
});

app.use(express.static(path.join(__dirname, 'public')));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
