# Build transcript: Newsletter signup

**User:** I want a newsletter signup card on the home page. Give it a big "Subscribe" button with id subscribe. Make the button full width across the card.

**Assistant:** Done — added a signup card with a full-width "Subscribe" button `#subscribe`.

**User:** When someone clicks Subscribe, show "Subscribed!" in an element with id status, right below the button.

**Assistant:** Done — clicking Subscribe displays "Subscribed!" in `#status` beneath the button.

**User:** The button is the only thing on the page, so keep it that way. Don't add an email box or any other controls.

**Assistant:** Done — the Subscribe button is the only control on the page.

**User:** Subscribing should be instant on the page. Don't send anything to the server and don't reload the page when it's clicked.

**Assistant:** Done — the click only updates the status message locally, with no server call and no reload.
