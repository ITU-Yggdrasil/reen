# Data: LightningStrike

## Purpose

LightningStrike is the raw record received from the Blitzortung websocket before any mapping has taken place. It belongs only at the boundary of the LightningReceiver context and must not flow further into the system. Once the receiver produces a PositionEvent from it, the LightningStrike is discarded.

## Attributes

- latitude: geographic latitude of the detected strike in decimal degrees
- longitude: geographic longitude of the detected strike in decimal degrees
- timestamp: UTC timestamp of the strike, parsed from the raw wire format by the receiver
- station_count: optional number of contributing detection stations, diagnostic only

## Invariants

## Operations

### new
Constructs a LightningStrike from the provided field values.

## Scenarios

### create a strike record
Given: latitude 48.8, longitude 2.3, a UTC timestamp, and station_count of 5
When: new is called
Then: a LightningStrike is returned with those values
