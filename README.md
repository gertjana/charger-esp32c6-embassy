# ESP32-C6 Embassy Charged

An ESP32-C6 based electric vehicle charging station built with Embassy async runtime and OCPP 1.6 support.

## Quick Start

### 1. Configuration Setup

Copy the example configuration file and update it with your settings:

```bash
cp app_config.toml.example app_config.toml
```

Edit `app_config.toml` with your actual values:

```toml
[wifi]
ssid = "YOUR_WIFI_NETWORK"
password = "YOUR_WIFI_PASSWORD"

[charger]
name = "esp32c6 charger 001"
model = "ESP32-C6"
vendor = "GA Make"
serial = "esp32c6-charger-001"

[mqtt]
broker = "broker.hivemq.com"
port = 1883
client_id = "esp32c6-charger-001"
```

### 2. Environment Variable Overrides (Optional)

You can override any configuration value using environment variables:

```bash
export CHARGER_WIFI_SSID="MyWiFiNetwork"
export CHARGER_WIFI_PASSWORD="MyWiFiPassword"
export CHARGER_NAME="my-charger-001"
export CHARGER_MODEL="ESP32-C6"
export CHARGER_VENDOR="My Company"
export CHARGER_SERIAL="my-charger-001"
export CHARGER_MQTT_BROKER="192.168.1.100"
export CHARGER_MQTT_PORT="1883"
export CHARGER_MQTT_CLIENT_ID="my-charger-001"
```

### 3. Build and Flash

```bash
cargo build
cargo run
```

## Configuration Reference

### WiFi Settings
- `ssid`: Your WiFi network name
- `password`: Your WiFi network password

### Charger Identity
- `name`: Human-readable charger name for identification
- `model`: Hardware model identifier (default: "ESP32-C6")
- `vendor`: Manufacturer or organization name
- `serial`: Unique serial number for this charger instance

### MQTT Connection
- `broker`: MQTT broker hostname or IP address
- `port`: MQTT broker port (default: 1883)
- `client_id`: Unique identifier for MQTT client connection

The charger automatically generates MQTT topics based on the serial number:
- Publishing topic: `/charger/{serial}`
- Subscription topic: `/system/{serial}`

## Hardware Connections

| Function | GPIO Pin | Description |
|----------|----------|-------------|
| Onboard LED | GPIO15 | Charging status indicator |
| Cable Switch | GPIO0 | Cable connection detector |
| Swipe Switch | GPIO1 | Card/authorization detector |
| Charger Relay | GPIO2 | Main charging relay control |

## OCPP Protocol Support

The charger implements OCPP 1.6 protocol with bidirectional MQTT communication:

### Outgoing Messages (Published to `/charger/{serial}`)
- **Heartbeat**: Periodic status updates every 30 seconds
- **BootNotification**: Sent once at startup with charger details
- **StatusNotification**: Charger state changes (Available, Preparing, Charging, etc.) (not yet)
- **StartTransaction**: Charging session initiation (not yet)
- **StopTransaction**: Charging session completion (not yet)

### Incoming Messages (Subscribed to `/system/{serial}`)
- **RemoteStartTransaction**: Cloud-initiated charging commands  (not yet)
- **RemoteStopTransaction**: Cloud-initiated stop commands (not yet)
- **Authorize**: Payment/authorization responses  (not yet)
- **OCPP Responses**: All CallResult and CallError responses to request messages implemented 

## Configuration Priority

Settings are applied in the following order (highest to lowest priority):

1. **Environment Variables** (highest priority)
2. **app_config.toml file**
3. **Built-in defaults** (fallback)

## Development

This project uses:
- **Embassy**: Async runtime for embedded Rust with concurrent task management
- **ESP-HAL**: Hardware abstraction for ESP32-C6
- **OCPP-RS**: Open Charge Point Protocol implementation
- **Embassy-Net**: Networking stack with WiFi and MQTT support
- **Rust-MQTT**: Lightweight MQTT client for embedded systems

### Architecture
The system is built around Embassy async tasks:
- **Network Stack**: WiFi connection management and IP configuration
- **MQTT Client**: Bidirectional message of OCPP Messages
- **NTP Client**: Queries NTP Server every 4 hours and syncing with local timer in the ESP32-C6
- **OCPP 1.6**: minimum support for OCPP 1.6 to support basic Charging behaviour
- **Hardware Tasks**: GPIO monitoring for cable detection, card swipes. Led and Relay control and update a small display
- **Periodic Tasks**: Heartbeat transmission and boot notifications

![Application Diagram](./architecture/app_diagram.svg)


### Memory Management
- **Heap Size**: 64KB allocated for dynamic memory
- **Message Buffers**: 2048-byte capacity for larger OCPP messages
- **Channel Queues**: 5-message capacity for MQTT send/receive operations
- **Static Allocation**: Embassy static cells for zero-allocation async runtime

## Security Note

The `app_config.toml` file contains sensitive information (WiFi passwords, etc.) and is excluded from version control. Always use the `.example` file as a template for new deployments.
