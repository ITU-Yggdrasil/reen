use chrono::{DateTime, Utc};
use tracing;

use crate::data::ledgerentry::LedgerEntry;

/// An immutable ledger that records committed transactions as a pair of
/// (LedgerEntry, commit_datetime_utc). Adding an entry produces a new Ledger,
/// preserving prior entries via the tail, with the newest entry at the head.
#[derive(Debug, Clone)]
pub struct Ledger {
    head: (LedgerEntry, DateTime<Utc>),
    tail: Option<Box<Ledger>>,
}

impl Ledger {
    /// Creates a new ledger with the provided entry as the head and no tail.
    /// The commit time is recorded as the current UTC time.
    pub fn new(entry: LedgerEntry) -> Self {
        let now = Utc::now();
        tracing::info!("[Ledger] new, commit_time_utc={}", now.to_rfc3339());
        Self {
            head: (entry, now),
            tail: None,
        }
    }

    /// Commits the given entry to the ledger, recording the current UTC time.
    /// Returns a new ledger whose head is the committed entry and whose tail
    /// is the previous ledger (this instance).
    pub fn add_entry(self, entry: LedgerEntry) -> Self {
        let now = Utc::now();
        tracing::info!("[Ledger] add_entry, commit_time_utc={}", now.to_rfc3339());
        Self {
            head: (entry, now),
            tail: Some(Box::new(self)),
        }
    }

    /// Returns all ledger entries where the provided account_id is either the sink or the source,
    /// sorted by their commit timestamps (ascending, oldest first).
    pub fn get_entries_for(&self, account_id: i64) -> Vec<LedgerEntry> {
        tracing::info!("[Ledger] get_entries_for, account_id={}", account_id);

        // Collect (entry, commit_time) pairs where the account participates
        let mut matched: Vec<(LedgerEntry, DateTime<Utc>)> = Vec::new();

        let mut current: Option<&Ledger> = Some(self);
        while let Some(ledger) = current {
            let (ref entry, ts) = ledger.head;

            let participates = entry
                .sink()
                .map(|id| id == account_id)
                .unwrap_or(false)
                || entry
                    .sourc_e()
                    .map(|id| id == account_id)
                    .unwrap_or(false);

            if participates {
                matched.push((entry.clone(), ts));
            }

            current = ledger.tail.as_deref();
        }

        // Sort by commit time ascending (oldest first)
        matched.sort_by_key(|(_, ts)| *ts);

        // Return only the entries, in sorted order
        matched.into_iter().map(|(e, _)| e).collect()
    }
}