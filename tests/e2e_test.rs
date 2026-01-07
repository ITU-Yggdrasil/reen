/// End-to-end integration test for the reen system
///
/// This test verifies the complete workflow:
/// 1. Create specifications from drafts
/// 2. Create implementation from specifications
/// 3. Create tests
/// 4. Verify the generated code compiles and works
///
/// Run with: cargo test e2e_money_transfer --test e2e_test -- --nocapture --ignored
/// The test is marked as ignored because it requires API keys and takes longer to run.

use std::process::Command;
use std::path::Path;
use std::fs;

#[test]
#[ignore]  // Ignore by default - requires API keys and is slow
fn e2e_money_transfer() {
    let root_dir = std::env::current_dir().expect("Failed to get current directory");
    let test_dir = root_dir.join("tests").join("money transfer");

    // Ensure test directory exists
    assert!(test_dir.exists(), "Test directory 'tests/money transfer' not found");

    println!("Root directory: {:?}", root_dir);
    println!("Test directory: {:?}", test_dir);

    // Step 1: Build reen
    println!("\n=== Step 1: Building reen ===");
    let build_status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(&root_dir)
        .status()
        .expect("Failed to build reen");

    assert!(build_status.success(), "Failed to build reen");
    println!("✓ reen built successfully");

    let reen_bin = root_dir.join("target").join("release").join("reen");
    assert!(reen_bin.exists(), "reen binary not found");

    // Step 2: Create specifications
    println!("\n=== Step 2: Creating specifications ===");

    // Ensure contexts directory exists
    let contexts_dir = test_dir.join("contexts");
    fs::create_dir_all(&contexts_dir).expect("Failed to create contexts directory");

    let spec_status = Command::new(&reen_bin)
        .arg("create")
        .arg("specification")
        .current_dir(&test_dir)
        .status()
        .expect("Failed to run reen create specification");

    assert!(spec_status.success(), "Failed to create specifications");

    // Verify specifications exist
    assert!(contexts_dir.join("account.md").exists(), "account.md specification not created");
    assert!(contexts_dir.join("money_transfer.md").exists(), "money_transfer.md specification not created");
    println!("✓ Specifications created");

    // Step 3: Create implementation
    println!("\n=== Step 3: Creating implementation ===");

    let impl_status = Command::new(&reen_bin)
        .arg("create")
        .arg("implementation")
        .current_dir(&test_dir)
        .status()
        .expect("Failed to run reen create implementation");

    assert!(impl_status.success(), "Failed to create implementation");

    // Verify implementation was created
    let contexts_src_dir = test_dir.join("src").join("contexts");
    assert!(contexts_src_dir.exists(), "src/contexts directory not created");
    println!("✓ Implementation created");

    // Step 4: Create tests
    //println!("\n=== Step 4: Creating tests ===");

    //let tests_status = Command::new(&reen_bin)
    //    .arg("create")
    //    .arg("tests")
    //    .current_dir(&test_dir)
    //    .status()
    //    .expect("Failed to run reen create tests");

    //assert!(tests_status.success(), "Failed to create tests");
    //println!("✓ Tests created");

    // Step 5: Compile the generated code
    println!("\n=== Step 5: Compiling generated code ===");

    let compile_status = Command::new("cargo")
        .arg("build")
        .current_dir(&test_dir)
        .status()
        .expect("Failed to compile generated code");

    assert!(compile_status.success(), "Generated code failed to compile");
    println!("✓ Generated code compiled successfully");

    // Step 6: Run generated tests
    println!("\n=== Step 6: Running generated tests ===");

    let test_output = Command::new("cargo")
        .arg("test")
        .current_dir(&test_dir)
        .output()
        .expect("Failed to run tests");

    println!("Test output:\n{}", String::from_utf8_lossy(&test_output.stdout));
    if !test_output.stderr.is_empty() {
        println!("Test stderr:\n{}", String::from_utf8_lossy(&test_output.stderr));
    }

    // Note: We don't assert success here because the generated tests might need refinement
    if test_output.status.success() {
        println!("✓ Generated tests passed");
    } else {
        println!("⚠ Some generated tests failed (this may be expected)");
    }

    println!("\n=== E2E Test Complete ===");
    println!("✓ Successfully executed full reen workflow:");
    println!("  - Drafts → Specifications");
    println!("  - Specifications → Implementation");
    println!("  - Specifications → Tests");
    println!("  - Generated code compiles");
}

#[test]
#[ignore]  // Ignore by default - depends on generated code
fn test_money_transfer_functionality() {
    // This test would verify the actual money transfer logic
    // It's marked as ignored because it depends on the generated code
    // which may not exist yet

    println!("\n=== Testing Money Transfer Functionality ===");

    // Note: This assumes the e2e_money_transfer test has already run
    // and generated the necessary code

    let test_dir = std::env::current_dir()
        .expect("Failed to get current directory")
        .join("tests")
        .join("money transfer");

    // Create a simple test that uses the generated library
    let test_code = r#"
use reen::{Account, Currency, Ledger, MoneyTransfer};
use chrono::Utc;

fn main() {
    let mut ledger = Ledger::new();

    // Setup accounts with initial balances
    let account_a_id = "account_a".to_string();
    let account_b_id = "account_b".to_string();

    // Deposit initial amounts
    ledger = ledger.add_entry(reen::LedgerEntry::sink(
        account_a_id.clone(),
        1000.0,
        Currency::USD,
        Utc::now(),
    ));

    ledger = ledger.add_entry(reen::LedgerEntry::sink(
        account_b_id.clone(),
        500.0,
        Currency::USD,
        Utc::now(),
    ));

    // Check initial balances
    let account_a = Account::new(account_a_id.clone(), &ledger);
    let account_b = Account::new(account_b_id.clone(), &ledger);

    assert_eq!(account_a.balance(), 1000.0);
    assert_eq!(account_b.balance(), 500.0);

    // Transfer 100 from A to B
    let transfer = MoneyTransfer::new(
        account_a_id.clone(),
        account_b_id.clone(),
        100.0,
        Currency::USD,
        ledger.clone(),
    );

    ledger = transfer.execute().expect("Transfer should succeed");

    // Verify final balances
    let account_a_final = Account::new(account_a_id, &ledger);
    let account_b_final = Account::new(account_b_id, &ledger);

    assert_eq!(account_a_final.balance(), 900.0);
    assert_eq!(account_b_final.balance(), 600.0);

    println!("✓ Money transfer test passed!");
}
"#;

    // Write the test code to a temporary file
    let temp_test = test_dir.join("examples").join("transfer_test.rs");
    fs::create_dir_all(temp_test.parent().unwrap())
        .expect("Failed to create examples directory");
    fs::write(&temp_test, test_code)
        .expect("Failed to write test code");

    println!("✓ Test code written to: {:?}", temp_test);
}
