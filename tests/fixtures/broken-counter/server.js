const express = require('express');
const app = express();
const port = process.env.PORT || 8091;

app.use(express.json());

let counter = 0;

app.get('/api/counter', (req, res) => {
    res.json({ value: counter });
});

app.post('/api/noop', (req, res) => {
    // Gap: This endpoint is wired to the UI but does nothing.
    res.json({ status: 'ok' });
});

app.get('/', (req, res) => {
    res.send(`
        <!DOCTYPE html>
        <html>
        <body>
            <h1>Broken Counter</h1>
            <div id="val">${counter}</div>
            <button id="inc-btn" onclick="
                window.__orchyn_logs = window.__orchyn_logs || [];
                window.__orchyn_logs.push('API Error: /api/counter/increment failed with 500');
                fetch('/api/noop', {method: 'POST'})
            ">Increment</button>
        </body>
        </html>
    `);
});

app.listen(port, () => {
    console.log(`Broken counter listening at http://localhost:${port}`);
});
