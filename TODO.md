# TODO List

## Current

### Tasks
* Include Accept/Reject from Start and StopTransaction responses into the statemachine
* TLS Connection for MQTT

### Bugs

* probably same/similar issue:
  * After running for a while (hours), the time shown on the display stops (app halts)
  * WHen time is not synced initially, the next sync halts the application or panics

## Archive

### tasks

* done ~~Subscribe/Receive MQTT messages~~
* done ~~Hook up SSD1306 display~~
* done ~~Use NTP Server to get the current date/time (no_std challenge)~~
* done ~~Time on display in local timezone~~
* done ~~Setup workflow swipe-> AuthorizeReq -> AuthorizeResp -> charging~~
* done ~~Implemented Lock handler~~
* done ~~Refactor de-bouncing into a util method~~
* done ~~set initial states based on cable inserted or not~~
* done ~~Start- and StopTransaction~~
* done ~~Clean up log and make it more consistent~~
* wont do ~~Proper (typesafe) Response Handler~~
* done ~~Implement code for RFID-RC522 SPI Module to do proper charge card swipes~~

### bugs

* ~~MQTT Topic should be specific for the charger (Id instead of name)~~
* ~~Resetting after error, should also evaluate the hardware to set the correct state~~
* ~~TransactionID doesn't seem to get stored correctly~~
* ~~Don't send StatusNofication on State Authorizing~~
* ~~Don't go to Faulted if no valid transition is found, keep the current state, no outputevents~~
