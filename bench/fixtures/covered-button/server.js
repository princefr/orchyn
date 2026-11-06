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
const OVERLAY = '<div class="scrim" aria-hidden="true"></div>';

const app = express();

// The page is rendered from a template so the overlay can be included only in the buggy build.
app.get(['/', '/index.html'], (req, res) => {
  res.type('html').send(template.replace('<!--OVERLAY-->', BUG ? OVERLAY : ''));
});

app.use(express.static(PUBLIC_DIR, { index: false }));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
