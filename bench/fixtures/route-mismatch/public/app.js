const listEl = document.getElementById('items');

function renderItems(items) {
  listEl.textContent = '';
  if (items.length === 0) {
    const li = document.createElement('li');
    li.textContent = 'No items';
    listEl.appendChild(li);
    return;
  }
  for (const name of items) {
    const li = document.createElement('li');
    li.textContent = name;
    listEl.appendChild(li);
  }
}

async function loadItems() {
  let items = [];
  try {
    const res = await fetch('/api/items');
    if (res.ok) items = await res.json();
  } catch (err) {
    items = [];
  }
  renderItems(items);
}

loadItems();
