/// Currency enum representing a subset of ISO 4217 currency codes.
/// 
/// Type Kind: Enum
/// Mutability: Immutable
/// 
/// Variants included based on the provided specification:
/// - USD
/// - EUR
/// - JPY
/// - GBP
/// - CNY
/// - AUD
/// - CAD
/// 
/// Notes on unspecified aspects:
/// - The complete list of possible values is not defined beyond those above.
/// - No validation or parsing functionalities are specified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    USD,
    EUR,
    JPY,
    GBP,
    CNY,
    AUD,
    CAD,
}