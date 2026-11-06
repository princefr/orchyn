const noteInput = document.getElementById('note');
const board = document.getElementById('board');

function render(notes) {
  board.textContent = '';
  for (const note of notes) {
    const div = document.createElement('div');
    div.className = 'note';
    div.textContent = note.text;
    board.appendChild(div);
  }
}

async function poll() {
  try {
    const res = await fetch('/api/notes');
    if (res.ok) render(await res.json());
  } catch (err) {
    // Network hiccup: try again on the next tick.
  }
}

document.getElementById('post-btn').addEventListener('click', async () => {
  const text = noteInput.value.trim();
  if (!text) return;
  const res = await fetch('/api/notes', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
  if (res.ok) {
    noteInput.value = '';
    await poll();
  }
});

poll();
setInterval(poll, 500);
