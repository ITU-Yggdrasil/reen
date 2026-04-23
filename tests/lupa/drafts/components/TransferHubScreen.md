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

### user opens the transfer hub
Given: authenticated user navigates to the transfer hub without a specific sub-path
When: the screen loads
Then: the own-account tab activates by default and the TransferBetweenOwnForm is visible

### user switches tabs and input is preserved
Given: the user has partially filled the DomesticTransferForm with recipient 1234-56789012 and amount 89,90 EUR
When: the user switches to the international tab and then back to the domestic tab
Then: the DomesticTransferForm still shows recipient 1234-56789012 and amount 89,90 EUR
