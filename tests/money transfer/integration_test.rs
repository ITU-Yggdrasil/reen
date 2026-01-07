use reen::{Account, Currency, Ledger, MoneyTransfer};
use chrono::Utc;

#[test]
fn test_transfer_between_accounts() {
    // Create a ledger
    let mut ledger = Ledger::new();

    // Create two accounts by depositing initial amounts
    let account_a_id = "account_a".to_string();
    let account_b_id = "account_b".to_string();

    // Deposit 1000 into account A (from unsettled source)
    let deposit_a = reen::LedgerEntry::sink(
        account_a_id.clone(),
        1000.0,
        Currency::USD,
        Utc::now(),
    );
    ledger = ledger.add_entry(deposit_a);

    // Deposit 500 into account B (from unsettled source)
    let deposit_b = reen::LedgerEntry::sink(
        account_b_id.clone(),
        500.0,
        Currency::USD,
        Utc::now(),
    );
    ledger = ledger.add_entry(deposit_b);

    // Verify initial balances
    let account_a = Account::new(account_a_id.clone(), &ledger);
    let account_b = Account::new(account_b_id.clone(), &ledger);

    println!("Initial balance A: {}", account_a.balance());
    println!("Initial balance B: {}", account_b.balance());

    assert_eq!(account_a.balance(), 1000.0, "Account A should start with 1000");
    assert_eq!(account_b.balance(), 500.0, "Account B should start with 500");

    // Transfer 100 from A to B
    let transfer = MoneyTransfer::new(
        account_a_id.clone(),
        account_b_id.clone(),
        100.0,
        Currency::USD,
        ledger.clone(),
    );

    match transfer.execute() {
        Ok(new_ledger) => {
            ledger = new_ledger;
            println!("Transfer successful!");
        }
        Err(e) => {
            panic!("Transfer failed: {:?}", e);
        }
    }

    // Verify final balances
    let account_a_final = Account::new(account_a_id.clone(), &ledger);
    let account_b_final = Account::new(account_b_id.clone(), &ledger);

    println!("Final balance A: {}", account_a_final.balance());
    println!("Final balance B: {}", account_b_final.balance());

    assert_eq!(account_a_final.balance(), 900.0, "Account A should have 900 after transfer");
    assert_eq!(account_b_final.balance(), 600.0, "Account B should have 600 after transfer");

    println!("✓ Transfer test passed!");
}
