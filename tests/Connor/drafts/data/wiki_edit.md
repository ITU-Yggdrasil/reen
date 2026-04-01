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

---

## Fields

wiki, title, user, timestamp, and namespace are readable by WikiEditReceiverContext when
deciding whether to process a record. coordinates is resolved externally and not present
in the raw event.

- **wiki**
  Which Wikimedia project this edit was made on (e.g., "enwiki", "dewiki", "frwikivoyage").
  Read by WikiEditReceiverContext when filtering: only edits to article namespaces of
  Wikipedia projects (those ending in "wiki", excluding "wikimedia") are processed.

- **namespace**
  The MediaWiki namespace of the edited page. Namespace 0 is the article namespace.
  Read by WikiEditReceiverContext when filtering: only namespace 0 edits are processed.

- **title**
  The title of the edited article as it appears in the wiki (spaces represented as
  underscores or spaces depending on the event format).
  Read by WikiEditReceiverContext when looking up the article's geographic coordinates
  via the GeoData API, and when setting the label of the resulting PositionEvent.

- **user**
  The username of the editor, or an IP address for anonymous edits. May be used for
  diagnostic purposes. Not used in the PositionEvent.

- **timestamp**
  The moment the edit was recorded by the Wikimedia servers, stored as a UTC timestamp.
  The receiver is responsible for converting from the ISO 8601 string in the wire format
  into a UTC timestamp before constructing the struct.
  Read by WikiEditReceiverContext when setting the occurred_at of the resulting PositionEvent.

- **bot**
  Whether the edit was flagged as a bot edit. May be used by the receiver to optionally
  filter out automated edits. Not included in the PositionEvent.
