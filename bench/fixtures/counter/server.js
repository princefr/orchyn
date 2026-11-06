const path = require('path');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

let value = 0;

const app = express();
app.use(express.json());

app.get('/api/counter', (req, res) => {
  res.json({ value });
});

app.post('/api/counter/inc', (req, res) => {
  value += BUG ? 2 : 1;
  res.json({ value });
});

app.use(express.static(path.join(__dirname, 'public')));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
