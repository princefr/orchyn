const path = require('path');
const crypto = require('crypto');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

const USERS = new Map([['alice@test.com', 'password123']]);
const sessions = new Map(); // token -> email

function parseCookies(header) {
  const out = {};
  (header || '').split(';').forEach((part) => {
    const i = part.indexOf('=');
    if (i > 0) out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  });
  return out;
}

function currentUser(req) {
  const token = parseCookies(req.headers.cookie).session;
  return token && sessions.has(token) ? sessions.get(token) : null;
}

const app = express();
app.use(express.json());

app.post('/api/login', (req, res) => {
  const { email, password } = req.body || {};
  if (!password || USERS.get(email) !== password) {
    return res.status(401).json({ error: 'Invalid credentials' });
  }
  const token = crypto.randomUUID();
  sessions.set(token, email);
  res.setHeader('Set-Cookie', `session=${token}; Path=/; HttpOnly; SameSite=Lax`);
  res.json({ ok: true, email });
});

app.get('/dashboard', (req, res) => {
  const user = BUG ? null : currentUser(req);
  res.type('html').send(`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Dashboard</title>
<link rel="icon" href="data:,">
</head>
<body>
<h1>Dashboard</h1>
<p id="greeting">${user ? `Welcome, ${user}` : 'Please sign in'}</p>
<a href="/">Back to login</a>
</body>
</html>`);
});

app.use(express.static(path.join(__dirname, 'public')));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
