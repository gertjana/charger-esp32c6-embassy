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
