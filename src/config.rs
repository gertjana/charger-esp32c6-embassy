extern crate alloc;
use alloc::format;

/// Configuration structure for the ESP32-C6 charger
#[derive(Clone, Debug)]
pub struct Config {
    pub wifi_ssid: &'static str,
    pub wifi_password: &'static str,
    pub charger_name: &'static str,
    pub charger_model: &'static str,
    pub charger_vendor: &'static str,
    pub charger_serial: &'static str,
    pub mqtt_broker: &'static str,
    pub mqtt_port: u16,
    pub ocpp_client_id: &'static str,
}

/// Simple TOML value extraction functions
fn extract_toml_string<'a>(content: &'a str, section: &str, key: &str) -> Option<&'a str> {
    // Find the section
    let section_marker = format!("[{section}]");
    let section_start = content.find(&section_marker)?;

    // Get content after section header
    let after_section = &content[section_start + section_marker.len()..];

    // Find the next section (or end of file)
    let section_end = after_section.find('[').unwrap_or(after_section.len());
    let section_content = &after_section[..section_end];

    // Look for the key in this section
    for line in section_content.lines() {
        let line = line.trim();
        if line.starts_with(key) && line.contains('=') {
            if let Some(eq_pos) = line.find('=') {
                let value = line[eq_pos + 1..].trim();
                // Remove quotes if present
                if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    return Some(&value[1..value.len() - 1]);
                }
                return Some(value);
            }
        }
    }
    None
}

fn extract_toml_integer(content: &str, section: &str, key: &str) -> Option<u16> {
    extract_toml_string(content, section, key)?.parse().ok()
}

impl Config {
    /// Create a new configuration from TOML file with environment variable overrides
    pub fn from_config() -> Self {
        // Include the TOML configuration at compile time
        const CONFIG_TOML: &str = include_str!("../app_config.toml");

        // Extract values from TOML with fallbacks
        let toml_wifi_ssid =
            extract_toml_string(CONFIG_TOML, "wifi", "ssid").unwrap_or("Wokwi-GUEST");
        let toml_wifi_password = extract_toml_string(CONFIG_TOML, "wifi", "password").unwrap_or("");
        let toml_charger_name =
            extract_toml_string(CONFIG_TOML, "charger", "name").unwrap_or("esp32c6-charger-001");
        let toml_mqtt_broker =
            extract_toml_string(CONFIG_TOML, "mqtt", "broker").unwrap_or("broker.hivemq.com");
        let toml_mqtt_port = extract_toml_integer(CONFIG_TOML, "mqtt", "port").unwrap_or(1883);
        let toml_ocpp_client_id =
            extract_toml_string(CONFIG_TOML, "mqtt", "client_id").unwrap_or("esp32c6-charger-001");

        Self {
            wifi_ssid: option_env!("CHARGER_WIFI_SSID").unwrap_or(toml_wifi_ssid),
            wifi_password: option_env!("CHARGER_WIFI_PASSWORD").unwrap_or(toml_wifi_password),
            charger_name: option_env!("CHARGER_NAME").unwrap_or(toml_charger_name),
            charger_model: option_env!("CHARGER_MODEL").unwrap_or("ESP32-C6"),
            charger_vendor: option_env!("CHARGER_VENDOR").unwrap_or("GA Make"),
            charger_serial: option_env!("CHARGER_SERIAL").unwrap_or("esp32c6-charger-001"),
            mqtt_broker: option_env!("CHARGER_MQTT_BROKER").unwrap_or(toml_mqtt_broker),
            mqtt_port: option_env!("CHARGER_MQTT_PORT")
                .and_then(|p| p.parse().ok())
                .unwrap_or(toml_mqtt_port),
            ocpp_client_id: option_env!("CHARGER_MQTT_CLIENT_ID").unwrap_or(toml_ocpp_client_id),
        }
    }

    /// Create a new configuration from environment variables (legacy method)
    pub fn from_env() -> Self {
        Self {
            wifi_ssid: option_env!("CHARGER_WIFI_SSID").unwrap_or("Wokwi-GUEST"),
            wifi_password: option_env!("CHARGER_WIFI_PASSWORD").unwrap_or(""),
            charger_name: option_env!("CHARGER_NAME").unwrap_or("esp32c6-charger-001"),
            charger_model: option_env!("CHARGER_MODEL").unwrap_or("ESP32-C6"),
            charger_vendor: option_env!("CHARGER_VENDOR").unwrap_or("GA Make"),
            charger_serial: option_env!("CHARGER_SERIAL").unwrap_or("esp32c6-charger-001"),
            mqtt_broker: option_env!("CHARGER_MQTT_BROKER").unwrap_or("broker.hivemq.com"),
            mqtt_port: option_env!("CHARGER_MQTT_PORT")
                .and_then(|p| p.parse().ok())
                .unwrap_or(1883),
            ocpp_client_id: option_env!("CHARGER_MQTT_CLIENT_ID").unwrap_or("esp32c6-charger-001"),
        }
    }

    /// MQTT topics
    pub fn charger_topic(&self) -> heapless::String<64> {
        let mut topic = heapless::String::new();
        topic.push_str("/charger/").ok();
        topic.push_str(self.charger_name).ok();
        topic
    }
    pub fn system_topic(&self) -> heapless::String<64> {
        let mut topic = heapless::String::new();
        topic.push_str("/system/").ok();
        topic.push_str(self.charger_name).ok();
        topic
    }

    /// Get OCPP message ID prefix
    pub fn ocpp_message_id_prefix(&self) -> &str {
        self.charger_name
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_config()
    }
}
