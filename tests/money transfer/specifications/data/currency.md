1. Description
The Currency type is an enum representing the Active ISO 4217 currency codes. The complete set of allowed codes is exactly the list provided in this specification.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Enum

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
None

5. Functionalities (only those explicitly named)
- to_str: Returns the three-letter code from the list of allowed ISO 4217 codes corresponding to the current enum variant.

6. Constraints & Rules (only those explicitly stated or directly implied)
- The set of valid enum variants is exactly the following codes (no others are permitted):
  AED, AFN, ALL, AMD, ANG, AOA, ARS, AUD, AWG, AZN, BAM, BBD, BDT, BGN, BHD, BIF, BMD, BND, BOB, BOV, BRL, BSD, BTN, BWP, BYN, BZD, CAD, CDF, CHE, CHF, CHW, CLF, CLP, CNY, COP, COU, CRC, CUP, CVE, CZK, DJF, DKK, DOP, DZD, EGP, ERN, ETB, EUR, FJD, FKP, GBP, GEL, GHS, GIP, GMD, GNF, GTQ, GYD, HKD, HNL, HTG, HUF, IDR, ILS, INR, IQD, IRR, ISK, JMD, JOD, JPY, KES, KGS, KHR, KMF, KPW, KRW, KWD, KYD, KZT, LAK, LBP, LKR, LRD, LSL, LYD, MAD, MDL, MGA, MKD, MMK, MNT, MOP, MRU, MUR, MVR, MWK, MXN, MXV, MYR, MZN, NAD, NGN, NIO, NOK, NPR, NZD, OMR, PAB, PEN, PGK, PHP, PKR, PLN, PYG, QAR, RON, RSD, RUB, RWF, SAR, SBD, SCR, SDG, SEK, SGD, SHP, SLE, SLL, SOS, SRD, SSP, STN, SVC, SYP, SZL, THB, TJS, TMT, TND, TOP, TRY, TTD, TWD, TZS, UAH, UGX, USD, USN, UYI, UYU, UYW, UZS, VED, VES, VND, VUV, WST, XAF, XAG, XAU, XBA, XBB, XBC, XBD, XCD, XDR, XOF, XPD, XPF, XPT, XSU, XTS, XUA, XXX, YER, ZAR, ZMW, ZWL
- Serialisation and deserialization should use serde aut-implementation.
- to_str returns the three-letter code exactly as written in the list above for the corresponding variant.

Inferred Types or Structures (Non-Blocking)
- Location: Currency (enum variants)
  - Inference made: Variants are unit-like (no associated data).
  - Basis for inference: The draft provides only a list of three-letter codes with no additional fields or data per variant.

Implementation Choices Left Open
- Non-blocking: The concrete string type returned by to_str (e.g., owned vs borrowed string) is not specified.
- Non-blocking: The serialization format(s) used with serde (e.g., JSON, YAML, etc.) and the exact representation per format are not specified.
- Non-blocking: Enum variant declaration order is not specified and has no stated semantic meaning.