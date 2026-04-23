# Component: AccountCard

## Purpose

Displays a summary of a single bank account: its human-readable name, current balance, masked account number, and account kind. Acts as a tappable entry point to the account's detail view.

## Props

- name: the user-assigned or bank-assigned account label
- balance: the current available balance, displayed using mono-numeric typography with the account's native currency symbol
- account-number: the full account number, displayed with the last four digits visible and the rest masked
- kind: the type of account, such as checking, savings, or investment

## Events

- account-selected: fired when the user taps the card, carries the account identifier

## Invariants

- Balance must always show exactly two decimal places for fiat currencies
- The account number must never be displayed in full; only the final four digits are visible

## Scenarios

- User views a checking account with a positive balance: the card shows the name, a balance in text-primary, the masked number, and a label indicating the kind
- User views a savings account with a zero balance: same layout, balance displays as zero with two decimal places
- User taps the card: account-selected fires with the account's identifier
