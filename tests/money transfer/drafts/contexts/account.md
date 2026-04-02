# Account

## Purpose

Account represents one ledger-backed account in the banking system.
Its behaviour is derived entirely from the immutable ledger and the account id it is bound to.

## Role Players

## Role Methods

## Props

| Prop | Meaning | Notes |
|---|---|---|
| account_id | Identifier of the account | Positive integer |
| ledger | Main ledger of the system | Read-only source of transactions |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| caller creating an account view | account_id, ledger | account context is created or an error is returned |

Rules:
- Accepts an account id and the ledger.
- At least one entry for the account must exist on the ledger.
- If no entry exists, returns an error.

| Given | When | Then |
|---|---|---|
| the ledger contains entries for account 42 | new is called for account 42 | an Account is returned |

### balance

| Started by | Uses | Result |
|---|---|---|
| caller requesting the current balance | ledger, account_id | balance amount is returned |

Rules:
- Balance is the sum of transactions where the account is sink minus the sum where the account is source.
- The result is an Amount object.

| Given | When | Then |
|---|---|---|
| the account has deposits and withdrawals on the ledger | balance is called | the returned Amount reflects sink minus source totals |

### get_currency

| Started by | Uses | Result |
|---|---|---|
| caller requesting account currency | ledger, account_id | account currency is returned |

Rules:
- All ledger entries for an account use the same currency.
- The currency is determined from entries where the account is source or sink.
- The currency of an account is immutable once established by the first ledger entry.

| Given | When | Then |
|---|---|---|
| the account already has ledger entries in one currency | get_currency is called | that currency is returned |

### account_id

| Started by | Uses | Result |
|---|---|---|
| caller requesting identity | account_id | account id is returned |

Rules:
- Returns the bound account id.

| Given | When | Then |
|---|---|---|
| an Account for id 42 | account_id is called | the result is 42 |

### transactions

| Started by | Uses | Result |
|---|---|---|
| caller requesting ledger history | ledger, account_id | related ledger entries are returned |

Rules:
- Returns all ledger entries related to the account.
- Results are sorted by transaction date in descending order.

| Given | When | Then |
|---|---|---|
| the account has multiple related ledger entries | transactions is called | the entries are returned newest first |
