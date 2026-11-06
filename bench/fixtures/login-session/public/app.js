const emailInput = document.getElementById('email');
const passwordInput = document.getElementById('password');
const errorEl = document.getElementById('error');

document.getElementById('login-btn').addEventListener('click', async () => {
  errorEl.textContent = '';
  const res = await fetch('/api/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email: emailInput.value, password: passwordInput.value }),
  });
  if (res.ok) {
    window.location.href = '/dashboard';
    return;
  }
  const body = await res.json().catch(() => ({}));
  errorEl.textContent = body.error || 'Login failed';
});
