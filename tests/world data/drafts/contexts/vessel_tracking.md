# Context: VesselTracking

## Purpose

Enacts the tracking of vessel positions by subscribing to the AISStream WebSocket feed and processing incoming position reports. The context coordinates a streaming connection with position-report handling to maintain a live view of vessel locations.

## Role Players

- stream: @aisstream/Subscribe — the streaming connection that delivers AIS messages
- position: @aisstream/PositionReport — a position report received from the stream

## Props

- api_key: the authentication key for the AISStream service
- bounding_box: the geographic area to monitor for vessel traffic

## Role Methods

### stream
- connect: establishes the WebSocket connection with the given credentials and bounding box, beginning the flow of AIS messages

### position
- extract_location: reads the vessel identifier, latitude, and longitude from the position report

## Functionalities

### track

Subscribes to the AIS stream via the stream's connect role method, then for each incoming message, calls the position's extract_location role method to obtain the vessel's current coordinates. Returns the extracted location data.

## Scenarios

### successful tracking session

Given: a valid api key and a bounding box covering the North Sea
When: track is invoked
Then: at least one vessel location with identifier, latitude, and longitude is returned
