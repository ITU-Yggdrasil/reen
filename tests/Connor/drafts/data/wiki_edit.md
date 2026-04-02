# WikiEdit

## Description

WikiEdit is the raw record as received from the Wikimedia RecentChange event stream before
any mapping has taken place. It belongs only at the boundary of the WikiEditReceiverContext
and must not flow further into the system.

The WikiEditReceiverContext is solely responsible for reading this record and producing a
PositionEvent from it. Once that mapping is done, the WikiEdit is discarded.

The Wikimedia RecentChange stream is a server-sent event (SSE) feed that emits one message
per change across all Wikimedia projects (Wikipedia, Wiktionary, Wikidata, etc.) in real
time. Most edits are to articles that have no geographic coordinates. Only edits to articles
that have geographic coordinates registered in the Wikimedia GeoData system will produce a
PositionEvent; all others are silently discarded.

The coordinates of an article represent the subject of the article, not the location of the
editor. An edit to the article for the Eiffel Tower produces a PositionEvent at Paris;
an edit to the article for the Amazon River produces an event in South America.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| wiki | Wikimedia project identifier such as `enwiki` | X | Used for project filtering and GeoData URL construction |
| namespace | MediaWiki namespace of the edited page | X | Namespace `0` is the article namespace |
| title | Title of the edited page | X | Used for GeoData lookup and PositionEvent label |
| user | Username or IP address of the editor |  | Diagnostic only |
| timestamp | UTC timestamp for when the edit was recorded | X | Receiver converts from the wire format |
| bot | Whether the edit was flagged as a bot edit | X | Used for optional bot filtering |
