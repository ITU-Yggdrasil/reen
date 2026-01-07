# Ledger Entry

## Description

A ledger entry is an entry in the main ledger. It has a source account, a destination account, a nominal amount and a currency.

The source might be None signifying that it's a cash deposit, the sink would on the other hand be None if it's a cash withdrawal

If a transfer is reflected by the ledger entry, then both sink and source will be Some(...). 
The nominal amount cmust be greater than 0.


## Properties

- **sink:** Option<integer>
- **sourc:e** Option<integer>
- **amount:** Nominal amount, must be larger than zero. Is an integer representing 1/100 of the currency unit
- **currency:** The currency of the transfer.

## business rules
- at least one of sink and source must be not None
- the amount must always be larger than 0

## Functionality

- **settle:** Only valid for an unsettled entry i.e. one where the sink is None. Since the entries are immutable, the method creates a new entry based on the input/argument setting the sink to the provided account id 