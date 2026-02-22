The primary application

Description
- A simple test application that transfers money between two fake accounts.

Initial state
- The application must add one ledger entry for each of the following accounts:
  - Account id 123456
  - Account id 654321
- For each account’s initial entry:
  - source is None
  - amount is 1000.00 DKK
  - The entry must comply with ledger and ledger entry business rules (at least one of sink/source is not None; amount > 0). Because source is None, the sink must be the corresponding account id.
- The effect of these initial entries is that each account is initialized with a 1000.00 DKK balance in the ledger.

Functionality
- Use the money transfer context to transfer 250.00 DKK from account 123456 (source) to account 654321 (sink).
- On successful completion of the transfer:
  - Print the account transactions on each of the accounts (account 123456 and account 654321) to standard output.
  - Each transaction printed corresponds to a ledger entry for the respective account.
- The application must exit with code 0 after printing the transactions.

Error handling
- In case of a runtime error at any point, the application must:
  - Exit with code 42 (a non-zero exit code)
  - If an error message is available, print it to standard error

Resolved From Dependencies (Non-Blocking)
- Account
  - transactions: “account transactions” are the set of all ledger entries related to the account, sorted by transaction date descending.
  - balance: used implicitly by the money transfer context’s withdraw business rule (cannot withdraw more than present balance).
  - currency: an account’s currency is immutable and defined by its first ledger entry. Initial entries in DKK establish DKK for each account.
  - new: constructing an Account requires at least one ledger entry for that account; satisfied after the initial state is established.
- Ledger and LedgerEntry
  - Ledger is immutable; adding an entry produces a new ledger and preserves the chain via prev_hash/hash. At least one of sink/source must be not None.
  - Ledger.create_entry produces an unsettled entry (sink None) and sets timestamp and prev_hash; Ledger.settle sets sink for unsettled entries. Either approach must obey the ledger’s chain integrity and business rules when creating initial entries and when committing the transfer entry.
  - Ledger.get_entries_for returns entries where the account is sink or source, sorted ascending by timestamp. If this is used instead of Account.transactions, the application must account for the different order (the draft does not mandate an order).
  - LedgerEntry.to_str returns a formatted textual representation of a ledger entry and can be used for printing each transaction.
- amount
  - A structured amount with currency; to_str renders “major.minor CURRENCY”. The draft specifies DKK for all amounts in this application.
- Money transfer (money transfer context)
  - new: constructs the context from source and sink account ids, amount, and ledger. Business rules include currency match between the amount and the source account.
  - source.withdraw: creates a withdrawal entry for the source; must fail if the amount exceeds the source’s balance.
  - sink.deposit: settles the withdrawal entry to the sink account.
  - Transfer: workflow is withdraw, then deposit, then add the resulting entry to the ledger; returns the resulting ledger on success. Any failure aborts with an error.

Implementation Choices Left Open (Non-Blocking)
- How the two initial entries are constructed and committed to the ledger (e.g., via create_entry + settle + add_entry or equivalent) as long as ledger entry and ledger chain rules are observed.
- Which account’s initial entry is added first and the exact timestamps of initial and transfer entries.
- Whether printing uses Account.transactions (descending) or Ledger.get_entries_for (ascending) and whether any additional ordering or transformation is applied; the draft only requires that all transactions for each account be printed.
- The exact surrounding formatting of the printed output beyond each transaction’s own string representation (e.g., headers, separators, or account labels) is unspecified.
- The internal representation of amounts in minor units and any currency enum details, beyond amounts being 1000.00 DKK for initialization and 250.00 DKK for the transfer.