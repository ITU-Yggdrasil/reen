The primary application

Description
- A simple test application that transfers money between two fake accounts.

Initial state
- Add ledger entries for two accounts: 123456 and 654321.
- For each account:
  - Create an initial entry where the source is None.
  - Set amount to 1000.
  - Set currency to DKK.
  - Add the entry to the main ledger.
- Per dependency-defined ledger entry semantics, an entry with source None represents a cash deposit; therefore the sink is the account’s id (see Resolved From Dependencies).

functionality
- Use the money transfer context to transfer 250 DKK from account 123456 to account 654321.
- After completing the transfer, print the account transactions for each of the two accounts to standard output:
  - Each transaction is a ledger entry.
  - For each transaction, print a line using the format:
    {date} - { entry.print() }
- Exit code should be 0 after successful completion.

Error handling
- In case of a runtime error, the application should exit with a non-0 exit code.
- The exit code should be 42.
- If an error message is available, it should be printed to standard error.

Resolved From Dependencies (Non-Blocking)
- ledgerentry (drafts/data/ledgerentry.md)
  - A ledger entry with source set to None represents a cash deposit; the sink should be set to the destination account id.
  - LedgerEntry.print is defined as: "{date} - {source id or none} - {sink id or none}:  {amount}{currency}" and includes the entry’s creation timestamp.
  - Amount is an integer representing 1/100 of a currency unit; it must be greater than 0. At least one of sink or source must be not None.
- currency (drafts/data/currency.md)
  - DKK is a valid ISO 4217 currency code in the Currency enum.
- Ledger (drafts/data/Ledger.md)
  - The ledger records the commit timestamp (UTC) for each committed entry and is immutable. It can return all entries for an account, sorted by commit timestamp.
- money_transfer (drafts/contexts/money_transfer.md)
  - The “money transfer context” provides a Transfer that withdraws from the source account and deposits to the sink, adding both ledger entries to the main ledger and returning Result<Ledger, String>.
  - Business rules include: same currency for source and sink; sufficient present balance; remaining_monthly_limit on the source account.
- account (drafts/contexts/account.md)
  - Account currency is immutable and established by the first ledger entry. Balance and remaining_monthly_limit are defined in terms of ledger transactions.

Unspecified or Ambiguous Aspects
- Amount units and conversion:
  - The initial amount “1000” and the transfer amount “250” are specified without clarifying whether they are whole currency units or minor units. Dependencies require ledger entry amounts as integer minor units and the money transfer amount as f64. The required conversion/rounding is not defined.
- Initial entry sink field:
  - The draft only specifies “source is left as none” for initial entries and does not explicitly state that sink must be the account id (this is inferred from dependency semantics).
- Initial ledger creation and commit order:
  - How the main ledger is instantiated before adding the initial entries is not specified.
  - The order in which the two initial entries are committed to the ledger is not specified.
- Printing details:
  - The required line format is "{date} - { entry.print() }", while LedgerEntry.print already includes a date. It is unspecified:
    - Which “date” the outer placeholder refers to (ledger commit timestamp vs. ledger entry creation timestamp).
    - The date/time format and timezone for any date(s).
    - Whether lines should contain two dates (outer date plus the date inside entry.print()) or whether the outer format replaces the inner date.
  - The order in which transactions are printed for each account is not specified.
  - The order in which the two accounts’ transaction lists are printed is not specified.
- Error classification:
  - It is not specified whether a business-rule failure returned by the money transfer context should be treated as a “runtime error” that triggers exit code 42 and printing to standard error.