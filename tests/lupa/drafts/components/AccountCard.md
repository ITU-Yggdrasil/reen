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

### user taps the card to view account detail
Given: an AccountCard displaying checking account "Salary" with balance 3 421,50 EUR and masked number **** 0785
When: the user taps the card
Then: account-selected fires carrying the account identifier for "Salary"

### zero balance renders with two decimal places
Given: an AccountCard displaying savings account "Rainy Day" with balance 0,00 EUR
When: the card renders
Then: the balance displays as "0,00 EUR" with two decimal places
