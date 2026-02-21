1. Description
The Currency type is an enum representing the Active ISO 4217 currency codes. The complete and exact set of enum variants is the list of codes provided below.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Enum

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- Variants (exactly this set):
  AED, AFN, ALL, AMD, ANG, AOA, ARS, AUD, AWG, AZN, BAM, BBD, BDT, BGN, BHD, BIF, BMD, BND, BOB, BOV, BRL, BSD, BTN, BWP, BYN, BZD, CAD, CDF, CHE, CHF, CHW, CLF, CLP, CNY, COP, COU, CRC, CUP, CVE, CZK, DJF, DKK, DOP, DZD, EGP, ERN, ETB, EUR, FJD, FKP, GBP, GEL, GHS, GIP, GMD, GNF, GTQ, GYD, HKD, HNL, HTG, HUF, IDR, ILS, INR, IQD, IRR, ISK, JMD, JOD, JPY, KES, KGS, KHR, KMF, KPW, KRW, KWD, KYD, KZT, LAK, LBP, LKR, LRD, LSL, LYD, MAD, MDL, MGA, MKD, MMK, MNT, MOP, MRU, MUR, MVR, MWK, MXN, MXV, MYR, MZN, NAD, NGN, NIO, NOK, NPR, NZD, OMR, PAB, PEN, PGK, PHP, PKR, PLN, PYG, QAR, RON, RSD, RUB, RWF, SAR, SBD, SCR, SDG, SEK, SGD, SHP, SLE, SLL, SOS, SRD, SSP, STN, SVC, SYP, SZL, THB, TJS, TMT, TND, TOP, TRY, TTD, TWD, TZS, UAH, UGX, USD, USN, UYI, UYU, UYW, UZS, VED, VES, VND, VUV, WST, XAF, XAG, XAU, XBA, XBB, XBC, XBD, XCD, XDR, XOF, XPD, XPF, XPT, XSU, XTS, XUA, XXX, YER, ZAR, ZMW, ZWL

5. Functionalities (only those explicitly named)
- to_str: &str
  - Returns the three letter acronym (the TLA) corresponding to the enum variant, taken from the list above.

6. Constraints & Rules (only those explicitly stated or directly implied)
- Serialization and deserialization should use serde auto-implementation (i.e., derive Serialize and Deserialize with default serde behavior for unit enum variants).
- The set of enum variants is exactly the list provided in this specification. No additional variants are included.

Unspecified or Ambiguous Aspects
- None identified in the draft.

Worth to Consider
- Non-blocking, out of scope: Providing conversions to/from string codes (e.g., FromStr/TryFrom<&str>, Display) for ergonomics.