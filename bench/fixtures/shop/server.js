const path = require('path');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

const ITEMS = ['Apples', 'Bread'];
const ITEMS_ROUTE = BUG ? '/api/item' : '/api/items';

const app = express();

app.get(ITEMS_ROUTE, (req, res) => {
  res.json(ITEMS);
});

app.use(express.static(path.join(__dirname, 'public')));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
