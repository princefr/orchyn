const fs = require('fs');
const path = require('path');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

const PUBLIC_DIR = path.join(__dirname, 'public');
const template = fs.readFileSync(path.join(PUBLIC_DIR, 'index.html'), 'utf8');
const BUG_CSS = '<style>#email-error { display: none !important; }</style>';

const app = express();
app.use(express.json());

app.get(['/', '/index.html'], (req, res) => {
  res.type('html').send(template.replace('<!--BUG_CSS-->', BUG ? BUG_CSS : ''));
});

app.post('/api/signup', (req, res) => {
  const email = String((req.body && req.body.email) || '').trim();
  if (!email) {
    return res.status(400).json({ error: 'Email is required' });
  }
  res.status(201).json({ ok: true, email });
});

app.use(express.static(PUBLIC_DIR, { index: false }));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
