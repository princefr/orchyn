const path = require('path');
const express = require('express');

const BUG = process.env.BUG === '1';
const PORT = process.env.PORT;
if (!PORT) {
  console.error('PORT env var is required');
  process.exit(1);
}

const app = express();

app.get('/api/profile', (req, res) => {
  res.json({
    name: 'Alice Chen',
    tags: BUG ? null : ['admin', 'beta'],
  });
});

app.use(express.static(path.join(__dirname, 'public')));

const server = app.listen(Number(PORT), () => {
  console.log(`listening on ${server.address().port}`);
});
