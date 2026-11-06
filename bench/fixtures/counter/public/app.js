const countEl = document.getElementById('count');

function render(value) {
  countEl.textContent = `Count: ${value}`;
}

fetch('/api/counter')
  .then((res) => res.json())
  .then((data) => render(data.value));

document.getElementById('inc').addEventListener('click', async () => {
  const res = await fetch('/api/counter/inc', { method: 'POST' });
  const data = await res.json();
  render(data.value);
});
