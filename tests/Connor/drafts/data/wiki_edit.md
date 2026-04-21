# Data: WikiEdit

## Purpose

WikiEdit is the raw record received from the Wikimedia RecentChange event stream before any mapping has taken place. It belongs only at the boundary of the WikiEditReceiver context and must not flow further into the system. Most edits are to articles without geographic coordinates and are discarded. Only edits to articles with coordinates registered in the Wikimedia GeoData system produce a PositionEvent. The coordinates represent the subject of the article, not the location of the editor.

## Attributes

- wiki: Wikimedia project identifier such as "enwiki", used for project filtering and GeoData URL construction
- namespace: MediaWiki namespace of the edited page; namespace 0 is the article namespace
- title: title of the edited page, used for GeoData lookup and as the PositionEvent label
- user: username or IP address of the editor, diagnostic only
- timestamp: UTC timestamp for when the edit was recorded, converted from the wire format by the receiver
- bot: whether the edit was flagged as a bot edit, used for optional bot filtering

## Invariants

## Operations

## Scenarios

### edit to a geotagged article
Given: wiki "enwiki", namespace 0, title "Eiffel Tower", timestamp as UTC, bot is false
When: the receiver resolves coordinates for "Eiffel Tower"
Then: a PositionEvent is produced at the coordinates of Paris

### edit outside article namespace
Given: wiki "enwiki", namespace 1 (talk page)
When: the receiver processes this edit
Then: the edit is discarded because only namespace 0 is eligible
