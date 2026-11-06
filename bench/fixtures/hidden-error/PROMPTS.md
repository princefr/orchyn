# Build transcript: Sign-up form

**User:** Make a simple sign-up page. An email box with id email and a "Sign up" button with id submit-btn.

**Assistant:** Done — added the email field `#email` and the "Sign up" button `#submit-btn`.

**User:** If someone hits Sign up with the email box empty, show "Email is required" in an element with id email-error, right under the email box.

**Assistant:** Done — an empty submit shows "Email is required" in `#email-error` directly below the field.

**User:** That message should go away as soon as they submit a real email.

**Assistant:** Done — the error message clears once a non-empty email is submitted.

**User:** When the email is filled in, send it to /api/signup. After that, show "Check your inbox to confirm." in an element with id result.

**Assistant:** Done — valid emails are posted to `/api/signup` and the confirmation message appears in `#result`.

**User:** And if the email box is empty, don't send anything to the server at all.

**Assistant:** Done — an empty email is blocked client-side and nothing is sent.
