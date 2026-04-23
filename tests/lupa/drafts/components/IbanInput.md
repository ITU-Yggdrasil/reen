# Component: IbanInput

## Purpose

Accepts an International Bank Account Number and validates its format progressively as the user types. Provides immediate feedback on structural correctness before any server-side check.

## Props

- value: the current text content of the field
- valid: read-only indicator of whether the current value passes structural validation

## Events

- iban-changed: fired on every keystroke, carries the current value and its validity status

## Invariants

- The component must accept input with or without space grouping and normalise it for display using standard four-character groups
- Validation must check country-code prefix, expected length for that country, and the mod-97 check digit
- The field must never emit a valid status for a structurally incorrect IBAN

## Scenarios

### user enters a valid IBAN
Given: an empty IbanInput
When: the user types DE89 3704 0044 0532 0130 00
Then: iban-changed fires with the normalised value and validity true, and the field displays the IBAN grouped in fours

### user enters an IBAN with incorrect check digit
Given: an empty IbanInput
When: the user types DE00 3704 0044 0532 0130 00
Then: iban-changed fires with validity false and the field shows an error state
