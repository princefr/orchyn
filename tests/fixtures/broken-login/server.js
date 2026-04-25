const express = require('express');
const app = express();
const port = process.env.PORT || 8092;

app.use(express.json());

app.post('/api/login', (req, res) => {
    // Gap: We set a cookie but the dashboard never checks it.
    res.cookie('session', 'valid-session-token');
    res.json({ status: 'ok' });
});

app.get('/dashboard', (req, res) => {
    // Gap: Dashboard ignores the cookie.
    res.send('<h1>Dashboard</h1><p>Please sign in to continue.</p>');
});

app.get('/', (req, res) => {
    res.send('<h1>Login</h1><form action="/api/login" method="POST"><button>Login</button></form>');
});

app.listen(port, () => {
    console.log(`Broken login listening at http://localhost:${port}`);
});
