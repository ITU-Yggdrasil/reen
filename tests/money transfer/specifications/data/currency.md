1. Description
- Currency is an enum whose variants represent the active ISO 4217 currency codes. The complete and exact set of variants is the list provided below. Serialization and deserialization use serde derive (auto-implementation).

2. Type Kind (Struct / Enum / NewType / Unspecified)
- Enum

3. Mutability (Immutable / Mutable)
- Immutable

4. Properties (only those explicitly mentioned)
- Variants (exactly as listed, unit-like):
  - AED, AFN, ALL, AMD, ANG, AOA, ARS, AUD, AWG, AZN, BAM, BBD, BDT, BGN, BHD, BIF, BMD, BND, BOB, BOV, BRL, BSD, BTN, BWP, BYN, BZD, CAD, CDF, CHE, CHF, CHW, CLF, CLP, CNY, COP, COU, CRC, CUP, CVE, CZK, DJF, DKK, DOP, DZD, EGP, ERN, ETB, EUR, FJD, FKP, GBP, GEL, GHS, GIP, GMD, GNF, GTQ, GYD, HKD, HNL, HTG, HUF, IDR, ILS, INR, IQD, IRR, ISK, JMD, JOD, JPY, KES, KGS, KHR, KMF, KPW, KRW, KWD, KYD, KZT, LAK, LBP, LKR, LRD, LSL, LYD, MAD, MDL, MGA, MKD, MMK, MNT, MOP, MRU, MUR, MVR, MWK, MXN, MXV, MYR, MZN, NAD, NGN, NIO, NOK, NPR, NZD, OMR, PAB, PEN, PGK, PHP, PKR, PLN, PYG, QAR, RON, RSD, RUB, RWF, SAR, SBD, SCR, SDG, SEK, SGD, SHP, SLE, SLL, SOS, SRD, SSP, STN, SVC, SYP, SZL, THB, TJS, TMT, TND, TOP, TRY, TTD, TWD, TZS, UAH, UGX, USD, USN, UYI, UYU, UYW, UZS, VED, VES, VND, VUV, WST, XAF, XAG, XAU, XBA, XBB, XBC, XBD, XCD, XDR, XOF, XPD, XPF, XPT, XSU, XTS, XUA, XXX, YER, ZAR, ZMW, ZWL

5. Functionalities (only those explicitly named)
- to_str
  - Returns the three-letter code corresponding to the variant (one of the codes listed above)

6. Constraints & Rules (only those explicitly stated or directly implied)
- Serialization and deserialization should use serde derive (auto-implementation). No additional customization is specified.
- The only valid values for this enum are the listed variants.

Implementation Choices Left Open
- Non-blocking:
  - Exact method signature details for to_str (e.g., concrete string type or borrowing semantics) are not specified, beyond returning the three-letter code.
  - Specifics of the serde-derived serialized form across formats (e.g., JSON vs. others) are not specified beyond using serde’s default derived behavior for unit variants.