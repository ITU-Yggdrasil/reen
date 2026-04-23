# Component: IbanInput

## Purpose

Accepts an International Bank Account Number and validates its format progressively as the user types. Provides immediate feedback on structural correctness before any server-side check.

## Props

- value: the current text content of the field
- valid: read-only indicator of whether the current value passes structural validation

## Events

- iban:changed: fired on every keystroke, carries the current value and its validity status

## Invariants

- The component must accept input with or without space grouping and normalise it for display using standard four-character groups
- Validation must check country-code prefix, expected length for that country, and the mod-97 check digit
- The field must never emit a valid status for a structurally incorrect IBAN

## Scenarios

- User types a correct IBAN character by character: the field groups characters into fours and reports valid once the full number passes the check-digit test
- User pastes a complete valid IBAN: the value normalises immediately and valid becomes true
- User enters an IBAN with an incorrect check digit: the field shows an error state and valid remains false
- User enters an IBAN with an unrecognised country code: the field shows an error state explaining the country is not supported
