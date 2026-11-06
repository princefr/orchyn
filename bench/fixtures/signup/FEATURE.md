# Sign-up form

- The home page `/` has an email field `#email` and a "Sign up" button `#submit-btn`.
- Submitting with the email field empty shows the message "Email is required" in `#email-error`, directly under the field.
- The message disappears once a non-empty email is submitted.
- A valid email is sent to `/api/signup`, and the page then shows "Check your inbox to confirm." in `#result`.
- Nothing is sent to the server when the email is empty.
