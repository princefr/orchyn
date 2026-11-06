const form = document.getElementById('signup-form');
const emailInput = document.getElementById('email');
const errorEl = document.getElementById('email-error');
const resultEl = document.getElementById('result');

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  resultEl.textContent = '';
  const email = emailInput.value.trim();

  if (!email) {
    errorEl.textContent = 'Email is required';
    errorEl.style.display = 'block';
    return;
  }

  errorEl.textContent = '';
  errorEl.style.display = 'none';
  const res = await fetch('/api/signup', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email }),
  });
  resultEl.textContent = res.ok ? 'Check your inbox to confirm.' : 'Something went wrong.';
});
