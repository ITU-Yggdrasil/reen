use std::process;

use tracing::{debug, error, info};

use chrono::Utc;

// Prefer crate-root re-exports
use crate::contexts::{Account, MoneyTransfer};
use crate::data::{Amount, Currency, Ledger, LedgerEntry};

pub struct ThePrimaryApplication;

impl ThePrimaryApplication {
    pub fn new() -> Self {
        info!("[ThePrimaryApplication] new");
        Self
    }

    pub fn run(&self) -> anyhow::Result<()> {
        info!("[ThePrimaryApplication] run");

        // 1) Initialize ledger with required initial entries
        let ledger = self.init_ledger()?;

        // 2) Execute transfer of 250.00 DKK from 123456 (source) to 654321 (sink)
        let transfer_amount = Amount::new(25_000, Currency::DKK)?;
        let mt = MoneyTransfer::new(654_321, 123_456, transfer_amount, ledger)?;
        let resulting_ledger = mt.transfer()?;

        // 3) On success, print account transactions for each account
        self.print_account_transactions(&resulting_ledger, 123_456)?;
        self.print_account_transactions(&resulting_ledger, 654_321)?;

        Ok(())
    }

    fn init_ledger(&self) -> anyhow::Result<Ledger> {
        debug!("[ThePrimaryApplication] init_ledger");

        // Initialize each account with a 1000.00 DKK balance
        let initial_amount = Amount::new(100_000, Currency::DKK)?;

        // First initial entry for account 123456
        // source=None, sink=Some(123456), amount=1000.00 DKK, prev_hash=None
        let entry1 = LedgerEntry::create(
            Some(123_456),
            None,
            initial_amount.clone(),
            Utc::now(),
            None,
        )?;
        let ledger = Ledger::new(entry1);

        // Second initial entry for account 654321 using the ledger API to ensure chain integrity
        // create_entry produces an unsettled entry (sink None), then settle to set sink to 654321, then add_entry
        let e2_unsettled = ledger.create_entry(None, initial_amount)?;
        let e2_settled = ledger.settle(&e2_unsettled, 654_321)?;
        let ledger = ledger.add_entry(e2_settled)?;

        Ok(ledger)
    }

    fn print_account_transactions(&self, ledger: &Ledger, account_id: i32) -> anyhow::Result<()> {
        debug!(
            "[ThePrimaryApplication] print_account_transactions, account_id={}",
            account_id
        );

        let account = Account::new(account_id, ledger)?;
        let txs = account.transactions();

        for entry in txs {
            let s = entry.to_str()?;
            println!("{}", s);
        }

        Ok(())
    }
}

fn main() {
    if let Err(e) = ThePrimaryApplication::new().run() {
        if !e.to_string().is_empty() {
            eprintln!("{}", e);
        }
        error!("[ThePrimaryApplication] runtime_error, error={}", e);
        process::exit(42);
    }
}