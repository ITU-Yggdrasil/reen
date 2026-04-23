# Component: CsrfFormField

## Purpose

A hidden form element that injects a CSRF protection token into any form submission. It is invisible to the user and requires no interaction.

## Props

- token: the CSRF token value provided by the server session

## Invariants

- The field must never be visible to the user
- The token must be included in every form submission that mutates server state
- If the token is absent, the parent form must prevent submission and surface an error

## Scenarios

- Form submits with a valid token: the token travels with the request payload and the server validates it
- Token is missing because the session expired: the parent form disables its submit control and displays a message asking the user to refresh
