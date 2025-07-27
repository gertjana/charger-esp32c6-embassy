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
name = "your-charger-id"

[mqtt]
broker = "your.mqtt.broker.com"
port = 1883
client_id = "your-charger-client-id"
```

### 2. Environment Variable Overrides (Optional)

You can override any configuration value using environment variables:

```bash
export CHARGER_WIFI_SSID="MyWiFiNetwork"
export CHARGER_WIFI_PASSWORD="MyWiFiPassword"
export CHARGER_NAME="my-charger-001"
export CHARGER_MQTT_BROKER="192.168.1.100"
export CHARGER_MQTT_PORT="1883"
export CHARGER_MQTT_CLIENT_ID="my-charger-001"
```

### 3. Build and Flash

```bash
cargo build
cargo run
```

## Hardware Connections

| Function | GPIO Pin | Description |
|----------|----------|-------------|
| Onboard LED | GPIO15 | Charging status indicator |
| Cable Switch | GPIO0 | Cable connection detector |
| Swipe Switch | GPIO1 | Card/authorization detector |
| Charger Relay | GPIO2 | Main charging relay control |

## OCPP Messages

The charger supports OCPP 1.6 protocol with the following messages:
- **Heartbeat**: Periodic status updates every 30 seconds
- **StatusNotification**: Charger state changes
- **StartTransaction**: Charging session initiation
- **StopTransaction**: Charging session completion

## Configuration Priority

Settings are applied in the following order (highest to lowest priority):

1. **Environment Variables** (highest priority)
2. **app_config.toml file**
3. **Built-in defaults** (fallback)

## Development

This project uses:
- **Embassy**: Async runtime for embedded Rust
- **ESP-HAL**: Hardware abstraction for ESP32-C6
- **OCPP-RS**: Open Charge Point Protocol implementation
- **Embassy-Net**: Networking stack with WiFi and MQTT support

## Security Note

The `app_config.toml` file contains sensitive information (WiFi passwords, etc.) and is excluded from version control. Always use the `.example` file as a template for new deployments.
