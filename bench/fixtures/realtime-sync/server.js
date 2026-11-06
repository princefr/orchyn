const path = require('path');
const crypto = require('crypto');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

const sharedNotes = [];
const notesBySession = new Map();

function parseCookies(header) {
  const out = {};
  (header || '').split(';').forEach((part) => {
    const i = part.indexOf('=');
    if (i > 0) out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  });
  return out;
}

const app = express();
app.use(express.json());

// Every browser gets a session id cookie so the server can tell visitors apart.
app.use((req, res, next) => {
  let sid = parseCookies(req.headers.cookie).sid;
  if (!sid) {
    sid = crypto.randomUUID();
    res.setHeader('Set-Cookie', `sid=${sid}; Path=/; HttpOnly; SameSite=Lax`);
  }
  req.sid = sid;
  next();
});

function notesFor(req) {
  if (!BUG) return sharedNotes;
  if (!notesBySession.has(req.sid)) notesBySession.set(req.sid, []);
  return notesBySession.get(req.sid);
}

app.get('/api/notes', (req, res) => {
  res.json(notesFor(req));
});

app.post('/api/notes', (req, res) => {
  const text = String((req.body && req.body.text) || '').trim();
  if (!text) {
    return res.status(400).json({ error: 'text required' });
  }
  const note = { id: crypto.randomUUID(), text, at: Date.now() };
  notesFor(req).push(note);
  res.status(201).json(note);
});

app.use(express.static(path.join(__dirname, 'public')));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
