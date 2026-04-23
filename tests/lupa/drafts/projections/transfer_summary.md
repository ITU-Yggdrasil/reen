# Projection: TransferSummary

## Purpose

Given a transfer request, produces a confirmation-ready text summary suitable for display in both the web UI and the chat agent's confirmation card.

## Source
- TransferRequest

## Queries
### summary
Returns a single text line describing the transfer: the amount with currency, the source account name, and the destination identifier. For own-account transfers, names both accounts. For domestic transfers, names the recipient and account number. For international transfers, names the recipient, IBAN, and BIC.

## Scenarios
### own-account transfer summary
Given: a transfer request of type own-account, five hundred kroner from "Salary" to "Savings"
When: summary is queried
Then: returns "500,00 DKK from Salary to Savings"

### domestic transfer summary
Given: a domestic transfer request of two hundred kroner to "Anders Jensen" at account number 1234-0056789012
When: summary is queried
Then: returns "200,00 DKK to Anders Jensen (1234-0056789012)"

### international transfer summary
Given: an international transfer of one thousand euros to "Maria Garcia" at IBAN ES12 3456 7890 1234 5678 90
When: summary is queried
Then: returns "1.000,00 EUR to Maria Garcia (ES12 3456 7890 1234 5678 90)"
