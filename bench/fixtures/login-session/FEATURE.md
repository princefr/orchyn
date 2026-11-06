# Login

- The sign-in page is at `/`. It has an email field `#email`, a password field `#password`, and a button labelled "Log in" (`#login-btn`).
- Logging in POSTs the credentials to `/api/login`. On success the browser moves to `/dashboard`.
- A wrong email or password shows "Invalid credentials" in `#error` and stays on the sign-in page.
- The dashboard at `/dashboard` greets a signed-in user with "Welcome, <email>" in `#greeting`.
- A visitor who is not signed in sees "Please sign in" on the dashboard.
- Test account: alice@test.com / password123.
