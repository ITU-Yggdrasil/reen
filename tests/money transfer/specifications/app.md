The primary application

Description
- Purpose: A simple test application that transfers money between two fake accounts and prints their transactions.

Initial state
- Create ledger entries for two accounts:
  - Account IDs: 123456 and 654321
  - Each entry is an initial entry with:
    - source: None
    - amount: 1000
    - currency: DKK

Functionality
- Use the money transfer context to transfer 250 DKK from account 123456 to account 654321.
- After completing the transfer, print the account transactions on each of the accounts to standard output.
  - Each transaction is a ledger entry.
  - The ledger entry has a print method.
  - Line format: "{date} - { entry.print() }"
- On successful completion, the application exits with code 0.

Error handling
- In case of a runtime error, the application must:
  - Exit with a non-zero exit code of 42
  - If an error message is available, print it to standard error

Resolved From Dependencies (Non-Blocking)
- Account
  - transactions returns all ledger entries related to the account, sorted by transaction date, descending. This defines the natural ordering of “account transactions” when printing, if Account.transactions is used.
  - account_id is a positive integer; the provided account IDs 123456 and 654321 satisfy this.
- amount
  - The numeric amount is stored in the minor unit of the currency. If the application constructs an amount value, the numeric value refers to minor units.
- Currency
  - DKK is a valid currency code; to_str would return "DKK".
- Ledger and LedgerEntry
  - A ledger entry must have at least one of sink or source not None, amount > 0, and includes a timestamp.
  - get_entries_for returns entries sorted ascending; Account.transactions (if used) is descending.
  - Ledger is immutable; adding an entry produces a new Ledger.
- Money transfer
  - The money transfer context encapsulates the transfer between source and sink accounts and exposes:
    - new: validates construction, including currency checks
    - execute: attempts the transfer and returns Result<Ledger, String>
  - Business rules (e.g., sufficient balance, limits, and matching currencies) are enforced within the context/roles.

Blocking Ambiguities
- Initial entry sink is unspecified
  - The draft states “source is left as none” for each initial entry but does not state the sink. Without a sink set to the specific account ID, a LedgerEntry cannot be associated with that account and would violate the rule that at least one of sink/source must be not None. This blocks constructing valid initial entries.
- Amount unit for “1000” and “250 DKK”
  - The amount type uses minor units. The draft gives bare numbers “1000” (initial entries) and “250 DKK” (transfer) without stating whether they are major or minor units. This impacts balances and printed values and must be defined to implement correctly.
- Print formatting and method name
  - The draft requires printing each transaction as "{date} - { entry.print() }" and refers to a “print method” on the ledger entry.
  - In dependencies, LedgerEntry defines to_str with a different format that already includes a timestamp. It is unclear whether entry.print() is the same as LedgerEntry.to_str(), and how to avoid duplicating the timestamp. The precise definition of “date” (e.g., full timestamp vs. date-only, format) is also not specified.
- Behavior on business-rule transfer failures
  - The draft defines exit behavior for “runtime error” (exit 42) but does not specify what to do if the money transfer context returns an error due to business rule violations (e.g., insufficient balance). It is unclear whether such errors are treated as runtime errors (exit 42 with message) or should be handled differently.

Implementation Choices Left Open (Non-Blocking)
- How the initial ledger entries are added (e.g., specific factory or commit sequence) is not specified and can follow any mechanism consistent with the immutable Ledger.
- Which API to use to obtain “account transactions” for printing (Account.transactions vs. Ledger.get_entries_for followed by sorting) is not mandated as long as the resulting set corresponds to the account’s transactions.
- Ordering of accounts when printing (whether to print 123456 first or 654321 first) is not specified.
- Output layout beyond the required per-transaction line format (e.g., blank lines or headers between accounts) is not specified.