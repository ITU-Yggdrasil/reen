# Component: TransferHubScreen

## Purpose

A tabbed screen that hosts the three transfer forms: own-account, domestic, and international. The active tab is determined by the current route path, allowing direct linking to a specific transfer type.

## Composition

- TransferBetweenOwnForm: shown when the own-account tab is active
- DomesticTransferForm: shown when the domestic tab is active
- InternationalTransferForm: shown when the international tab is active

## Invariants

- The screen requires an authenticated session; unauthenticated visitors must be redirected to LoginScreen
- Exactly one tab is active at any time
- Switching tabs must not discard unsaved input in the previously active form; values persist for the duration of the session
- The active tab must stay synchronised with the current route path

## Scenarios

- User navigates to the transfer hub without a specific sub-path: the own-account tab activates by default
- User selects the domestic tab: the route updates and the DomesticTransferForm becomes visible
- User completes a transfer in any form: a success message appears and the form resets
- User switches from a partially filled form to another tab and back: the original form retains its values
