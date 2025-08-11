# List of tasks

* Clean up log and make it more consistent
* Implement code for RFID-RC522 SPI Module to do proper charge card swipes
* Proper (typesafe) Response Handler
* TLS Connection for MQTT


## Bugs

* ~~MQTT Topic should be specific for the charger (Id instead of name)~~
* ~~Resetting after error, should also evaluate the hardware to set the correct state~~
* ~~TransactionID doesn't seem to get stored correctly~~
* ~~Don't send StatusNofication on State Authorizing~~
* probably same/similar issue:
  * After running for a while (hours), the time shown on the display stops (app halts)
  * WHen time is not synced initially, the next sync halts the application or panics


