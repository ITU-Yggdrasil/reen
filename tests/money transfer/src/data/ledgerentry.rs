use crate::types::currency::Currency;
use tracing;

/// Errors that can occur when settling a LedgerEntry
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettleError {
    /// The entry already has a sink set (i.e., it is already settled)
    AlreadySettled,
    /// The entry is invalid because both sink and source are None
    InvalidParticipants,
    /// The amount on the entry is zero, which is invalid (must be larger than zero)
    InvalidAmountZero,
    /// The provided sink account id is zero, which is invalid (must be a positive integer)
    InvalidSinkAccountId,
}

/// A ledger entry recording a movement in the main ledger.
///
/// Notes regarding field names:
/// - The specification lists a property named "sourc:e". Since ":" is not a valid Rust identifier
///   character, this implementation uses the field name `sourc_e` to represent that property exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    /// The destination account (None signifies a cash withdrawal)
    sink: Option<u64>,
    /// The source account (None signifies a cash deposit)
    ///
    /// This corresponds to the "sourc:e" property in the specification.
    sourc_e: Option<u64>,
    /// The nominal amount, integer representing 1/100 of the currency unit; must be larger than zero
    amount: u64,
    /// The currency of the entry
    currency: Currency,
}

impl LedgerEntry {
    /// Settle an unsettled entry by providing the sink (destination) account id.
    ///
    /// Rules enforced:
    /// - Only valid when `sink` is None (unsettled). Otherwise, returns `AlreadySettled`.
    /// - `amount` must be larger than 0 (returns `InvalidAmountZero` if zero).
    /// - At least one of `sink` or `sourc_e` must be not None. Given `sink` is None for unsettled,
    ///   this requires `sourc_e` to be Some(...) (returns `InvalidParticipants` otherwise).
    /// - The provided sink account id must be a positive integer (returns `InvalidSinkAccountId` if 0).
    ///
    /// Behavior:
    /// - Creates a new entry based on the current one, setting `sink` to the provided account id.
    pub fn settle(&self, sink_account_id: u64) -> Result<LedgerEntry, SettleError> {
        tracing::info!(
            "[LedgerEntry] settle, sink_account_id={}, current_sink={:?}, current_source={:?}, amount={}, currency={:?}",
            sink_account_id,
            self.sink,
            self.sourc_e,
            self.amount,
            self.currency
        );

        if self.sink.is_some() {
            tracing::error!("[LedgerEntry] settle, already_settled");
            return Err(SettleError::AlreadySettled);
        }
        if self.amount == 0 {
            tracing::error!("[LedgerEntry] settle, invalid_amount_zero");
            return Err(SettleError::InvalidAmountZero);
        }
        // With sink == None (required for unsettled), ensure that at least one participant is present.
        if self.sourc_e.is_none() {
            tracing::error!("[LedgerEntry] settle, invalid_participants_both_none");
            return Err(SettleError::InvalidParticipants);
        }
        if sink_account_id == 0 {
            tracing::error!("[LedgerEntry] settle, invalid_sink_account_id_zero");
            return Err(SettleError::InvalidSinkAccountId);
        }

        let new_entry = LedgerEntry {
            sink: Some(sink_account_id),
            sourc_e: self.sourc_e,
            amount: self.amount,
            currency: self.currency.clone(),
        };

        tracing::info!(
            "[LedgerEntry] settle, settled_sink={:?}, source={:?}",
            new_entry.sink,
            new_entry.sourc_e
        );

        Ok(new_entry)
    }
}