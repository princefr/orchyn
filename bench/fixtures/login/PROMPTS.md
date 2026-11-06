# Build transcript: Login

**User:** I want a simple login app. A sign-in page on the home page with an email box, a password box and a button that says "Log in". Give the email box the id email and the password box the id password, and the button id login-btn.

**Assistant:** Done — added the sign-in page at `/` with the email field, password field and "Log in" button.

**User:** When someone logs in, send the email and password to /api/login. If it works, take them to /dashboard.

**Assistant:** Done — login now posts to `/api/login` and redirects to `/dashboard` on success.

**User:** If the email or password is wrong, show "Invalid credentials" in a box with id error, and keep them on the sign-in page. Make sure they don't get moved anywhere in that case.

**Assistant:** Done — wrong credentials show "Invalid credentials" in `#error` and the user stays on the sign-in page.

**User:** The dashboard should say "Welcome, " followed by whoever is logged in, in an element with id greeting. Put their email after it.

**Assistant:** Done — the dashboard greets the signed-in user with "Welcome, <email>" in `#greeting`.

**User:** If someone opens /dashboard without being signed in, it should just say "Please sign in". It should never show the greeting to them.

**Assistant:** Done — visitors who aren't signed in see "Please sign in" on the dashboard instead of the greeting.

**User:** For trying it out, I've got a test account alice@test.com with password123. Make sure that one works.

**Assistant:** Done — alice@test.com / password123 signs in successfully.
