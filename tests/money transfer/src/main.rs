use std::process;

mod contexts;
mod types;

use contexts::{Account, MoneyTransfer};
use types::{Currency, Ledger, LedgerEntry};

fn main() {
    if let Err(err) = run() {
        if !err.is_empty() {
            eprintln!("{}", err);
        }
        process::exit(42);
    }
    process::exit(0);
}

fn run() -> Result<(), String> {
    // Initial state setup
    let account1_id: i64 = 123456;
    let account2_id: i64 = 654321;

    // Interpret "1000" as 1000 whole currency units (DKK), convert to minor units (1/100)
    let initial_amount_minor = 1000_i64
        .checked_mul(100)
        .ok_or_else(|| "Amount overflow when converting to minor units".to_string())?;

    let initial_entry_1 = LedgerEntry::new(
        None,                           // source: None signifies a cash deposit
        Some(account1_id),              // sink: account 123456
        initial_amount_minor,           // amount in minor units
        Currency::DKK,
    )?;

    // Create a new ledger with the first initial entry
    let mut ledger = Ledger::new(initial_entry_1);

    // Add the second initial entry for account 654321
    let initial_entry_2 = LedgerEntry::new(
        None,                           // source: None signifies a cash deposit
        Some(account2_id),              // sink: account 654321
        initial_amount_minor,           // amount in minor units
        Currency::DKK,
    )?;
    ledger = ledger.add_entry(initial_entry_2);

    // Construct accounts bound to the current main ledger
    let source_account = Account::new(account1_id.to_string(), ledger.clone());
    let sink_account = Account::new(account2_id.to_string(), ledger.clone());

    // Execute money transfer: 250 DKK from 123456 to 654321
    let transfer = MoneyTransfer::new(source_account, sink_account, 250.0_f64, Currency::DKK, ledger)?;
    let updated_ledger = transfer.execute()?;

    // After completing the transfer, print the account transactions for each account
    print_account_transactions(&updated_ledger, account1_id)?;
    print_account_transactions(&updated_ledger, account2_id)?;

    Ok(())
}

fn print_account_transactions(ledger: &Ledger, account_id: i64) -> Result<(), String> {
    // Expect get_entries_for to return entries where this account is either sink or source,
    // sorted by commit timestamp (ascending or defined by implementation).
    let entries = ledger.get_entries_for(account_id);

    for (entry, date) in entries {
        // Each line: {date} - { entry.print() }
        println!("{} - {}", date, entry.print());
    }

    Ok(())
}