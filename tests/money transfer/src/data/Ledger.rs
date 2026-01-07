use std::time::SystemTime;
use tracing;

/// A simple ledger entry representing a transfer from a source account to a sink account,
/// with an amount. This type is provided here due to unspecified external structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    source_account: String,
    sink_account: String,
    amount: i64,
}

impl LedgerEntry {
    /// Constructs a new LedgerEntry.
    pub fn new(source_account: String, sink_account: String, amount: i64) -> Self {
        tracing::info!(
            "[LedgerEntry] new, source_account={}, sink_account={}, amount={}",
            source_account,
            sink_account,
            amount
        );
        Self {
            source_account,
            sink_account,
            amount,
        }
    }

    /// Returns the source account of this entry.
    pub fn source_account(&self) -> &str {
        tracing::info!(
            "[LedgerEntry] source_account, source_account={}",
            self.source_account
        );
        &self.source_account
    }

    /// Returns the sink account of this entry.
    pub fn sink_account(&self) -> &str {
        tracing::info!(
            "[LedgerEntry] sink_account, sink_account={}",
            self.sink_account
        );
        &self.sink_account
    }

    /// Returns the amount of this entry.
    pub fn amount(&self) -> i64 {
        tracing::info!("[LedgerEntry] amount, amount={}", self.amount);
        self.amount
    }
}

/// The main ledger structure. Immutable persistent structure; adding an entry returns
/// a new Ledger instance with the previous ledger as its tail.
#[derive(Debug)]
pub struct Ledger<'a> {
    head: (LedgerEntry, SystemTime),
    tail: Option<&'a Ledger<'a>>,
}

impl<'a> Ledger<'a> {
    /// Provided an account number, returns all ledger entries where that account is either
    /// source or sink, sorted by the timestamps of when they were committed (oldest first).
    pub fn get_entries_for(&self, account: &str) -> Vec<LedgerEntry> {
        tracing::info!("[Ledger] get_entries_for, account={}", account);

        let mut collected: Vec<(LedgerEntry, SystemTime)> = Vec::new();

        // Traverse the immutable chain
        let mut cursor: Option<&Ledger<'a>> = Some(self);
        while let Some(ledger) = cursor {
            let (ref entry, committed_at) = ledger.head;
            if entry.source_account == account || entry.sink_account == account {
                collected.push((entry.clone(), committed_at));
            }
            cursor = ledger.tail;
        }

        // Sort by commit time ascending
        collected.sort_by_key(|(_, committed_at)| committed_at.clone());

        // Return only the entries, in the sorted order
        collected.into_iter().map(|(entry, _)| entry).collect()
    }

    /// Commits an entry to the main ledger and records the commit time.
    /// The result is a new ledger with the provided previous ledger as its internal tail.
    ///
    /// Usage:
    /// - To append to an existing ledger: Ledger::add_entry(Some(&existing), entry)
    /// - To create the first ledger entry: Ledger::add_entry(None, entry)
    pub fn add_entry(tail: Option<&'a Ledger<'a>>, entry: LedgerEntry) -> Ledger<'a> {
        tracing::info!(
            "[Ledger] add_entry, tail_present={}, source_account={}, sink_account={}, amount={}",
            tail.is_some(),
            entry.source_account,
            entry.sink_account,
            entry.amount
        );

        Ledger {
            head: (entry, SystemTime::now()),
            tail,
        }
    }
}