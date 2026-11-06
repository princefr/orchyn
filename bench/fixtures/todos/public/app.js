const titleInput = document.getElementById('title');
const listEl = document.getElementById('list');

async function loadTodos() {
  const res = await fetch('/api/todos');
  const todos = await res.json();
  listEl.textContent = '';
  for (const todo of todos) {
    const li = document.createElement('li');
    li.textContent = todo.title;
    listEl.appendChild(li);
  }
}

document.getElementById('add-btn').addEventListener('click', async () => {
  const title = titleInput.value.trim();
  if (!title) return;
  await fetch('/api/todos', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ title }),
  });
  titleInput.value = '';
  await loadTodos();
});

loadTodos();
