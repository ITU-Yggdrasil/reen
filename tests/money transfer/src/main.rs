use std::process;

fn main() {
    let msg = "ERROR: Cannot implement specification as written.

Problem: The application must create initial ledger entries, but the public API from dependencies does not provide a way to construct the first LedgerEntry or an initial Ledger without an existing head entry. Ledger::create_entry requires a current head entry to supply prev_hash, and LedgerEntry::create is specified as pub(crate), making it inaccessible to the application binary. Ledger::new requires an already-created entry.

Required:
- Provide a public factory to create the first (genesis) entry or an empty Ledger, or
- Make LedgerEntry::create public, or
- Provide a Ledger::bootstrap or Ledger::create_genesis_entry function, or
- Provide another documented path for constructing the initial ledger entries from the application.

Once clarified, the application can initialize the two accounts, perform the transfer, and print transactions as specified.";
    eprintln!("{}", msg);
    process::exit(42);
}