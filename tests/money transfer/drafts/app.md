# The primary application


## Description
This is a simple test applications, transfering money between two fake accounts

### Initial state
It should add ledget entries for two accounts. The account ids are 123456 and 654321. The entry for each should be an initial entry. I.e. source is left as none. The amount should be 1000.00 DKK.


### functionality
The simple test case is to use the "money transfer context" to transfer 250.00 DKK from account 123456 to account 654321. After completing the transfer it should print the account transactions on each of the accounts to standard output. Each transaction is a ledger entry.
The exit code should be 0

### Error handling
In case of a runtime error the application should exit with a non 0 exit code. The exit code should be 42 and if an error message is available it should be printed to standard error