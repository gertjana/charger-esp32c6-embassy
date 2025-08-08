# List of tasks

* ~~Subscribe/Receive MQTT messages~~
* ~~Hook up SSD1306 display~~
* ~~Use NTP Server to get the current date/time (no_std challenge)~~
* ~~Time on display in local timezone~~
* ~~Setup workflow swipe-> AuthorizeReq -> AuthorizeResp -> charging~~
* ~~Implemented Lock handler~~
* Refactor de-bouncing into a util metho
* Start- and StopTransaction
* Clean up log and make it more consistent
* Proper (typesafe) Response Handler
* TLS Connection for MQTT


## Bugs

* ~~MQTT Topic should be specific for the charger (Id instead of name)~~
* probably same/similar issue
  * After running for a while (hours), the time shown on the display stops (app halts)
  * WHen time is not synced initially, the next sync halts the application or panics

