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
    pub mqtt_client_id: &'static str,
    pub ntp_server: &'static str,
    pub ntp_sync_interval_minutes: u16, // NTP sync interval in minutes
    pub timezone_offset_hours: i8, // Timezone offset from UTC in hours (e.g., +1 for CET, -5 for EST)
    pub ocpp_heartbeat_interval: u16, // Heartbeat interval in seconds
    pub ocpp_id_tag: &'static str, // OCPP ID tag for authorization and transactions
}

fn extract_toml_string<'a>(content: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let section_marker = format!("[{section}]");
    let section_start = content.find(&section_marker)?;

    let after_section = &content[section_start + section_marker.len()..];

    let section_end = after_section.find('[').unwrap_or(after_section.len());
    let section_content = &after_section[..section_end];

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
    pub fn from_config() -> Self {
        // Include the TOML configuration at compile time
        const CONFIG_TOML: &str = include_str!("../app_config.toml");

        let toml_wifi_ssid =
            extract_toml_string(CONFIG_TOML, "wifi", "ssid").unwrap_or("Wokwi-GUEST");
        let toml_wifi_password = extract_toml_string(CONFIG_TOML, "wifi", "password").unwrap_or("");
        let toml_charger_name =
            extract_toml_string(CONFIG_TOML, "charger", "name").unwrap_or("esp32c6 charger 001");
        let toml_charger_model =
            extract_toml_string(CONFIG_TOML, "charger", "model").unwrap_or("ESP32-C6");
        let toml_charger_vendor =
            extract_toml_string(CONFIG_TOML, "charger", "vendor").unwrap_or("GA Make");
        let toml_charger_serial =
            extract_toml_string(CONFIG_TOML, "charger", "serial").unwrap_or("esp32c6-charger-001");
        let toml_mqtt_broker =
            extract_toml_string(CONFIG_TOML, "mqtt", "broker").unwrap_or("broker.hivemq.com");
        let toml_mqtt_port = extract_toml_integer(CONFIG_TOML, "mqtt", "port").unwrap_or(1883);
        let toml_mqtt_client_id =
            extract_toml_string(CONFIG_TOML, "mqtt", "client_id").unwrap_or("esp32c6-charger-001");
        let toml_ntp_server =
            extract_toml_string(CONFIG_TOML, "ntp", "server").unwrap_or("pool.ntp.org");
        let toml_ntp_sync_interval_minutes =
            extract_toml_integer(CONFIG_TOML, "ntp", "sync_interval_minutes").unwrap_or(240);
        let toml_timezone_offset =
            extract_toml_integer(CONFIG_TOML, "display", "timezone_offset_hours")
                .map(|offset| offset as i8)
                .unwrap_or(0);
        let toml_heartbeat_interval =
            extract_toml_integer(CONFIG_TOML, "ocpp", "heartbeat_interval").unwrap_or(900);
        let toml_ocpp_id_tag =
            extract_toml_string(CONFIG_TOML, "ocpp", "id_tag").unwrap_or("123456");

        Self {
            wifi_ssid: option_env!("CHARGER_WIFI_SSID").unwrap_or(toml_wifi_ssid),
            wifi_password: option_env!("CHARGER_WIFI_PASSWORD").unwrap_or(toml_wifi_password),
            charger_name: option_env!("CHARGER_NAME").unwrap_or(toml_charger_name),
            charger_model: option_env!("CHARGER_MODEL").unwrap_or(toml_charger_model),
            charger_vendor: option_env!("CHARGER_VENDOR").unwrap_or(toml_charger_vendor),
            charger_serial: option_env!("CHARGER_SERIAL").unwrap_or(toml_charger_serial),
            mqtt_broker: option_env!("CHARGER_MQTT_BROKER").unwrap_or(toml_mqtt_broker),
            mqtt_port: option_env!("CHARGER_MQTT_PORT")
                .and_then(|p| p.parse().ok())
                .unwrap_or(toml_mqtt_port),
            mqtt_client_id: option_env!("CHARGER_MQTT_CLIENT_ID").unwrap_or(toml_mqtt_client_id),
            ntp_server: option_env!("CHARGER_NTP_SERVER").unwrap_or(toml_ntp_server),
            ntp_sync_interval_minutes: option_env!("CHARGER_NTP_SYNC_INTERVAL_MINUTES")
                .and_then(|interval| interval.parse().ok())
                .unwrap_or(toml_ntp_sync_interval_minutes),
            timezone_offset_hours: option_env!("CHARGER_TIMEZONE_OFFSET_HOURS")
                .and_then(|offset| offset.parse().ok())
                .unwrap_or(toml_timezone_offset),
            ocpp_heartbeat_interval: option_env!("CHARGER_OCPP_HEARTBEAT_INTERVAL")
                .and_then(|interval| interval.parse().ok())
                .unwrap_or(toml_heartbeat_interval),
            ocpp_id_tag: option_env!("CHARGER_OCPP_ID_TAG").unwrap_or(toml_ocpp_id_tag),
        }
    }

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
            mqtt_client_id: option_env!("CHARGER_MQTT_CLIENT_ID").unwrap_or("esp32c6-charger-001"),
            ntp_server: option_env!("CHARGER_NTP_SERVER").unwrap_or("pool.ntp.org"),
            ntp_sync_interval_minutes: option_env!("CHARGER_NTP_SYNC_INTERVAL_MINUTES")
                .and_then(|interval| interval.parse().ok())
                .unwrap_or(240),
            timezone_offset_hours: option_env!("CHARGER_TIMEZONE_OFFSET_HOURS")
                .and_then(|offset| offset.parse().ok())
                .unwrap_or(0),
            ocpp_heartbeat_interval: option_env!("CHARGER_OCPP_HEARTBEAT_INTERVAL")
                .and_then(|interval| interval.parse().ok())
                .unwrap_or(900),
            ocpp_id_tag: option_env!("CHARGER_OCPP_ID_TAG").unwrap_or("123456"),
        }
    }

    pub fn charger_topic(&self) -> heapless::String<64> {
        let mut topic = heapless::String::new();
        topic.push_str("/charger/").ok();
        topic.push_str(self.charger_serial).ok();
        topic
    }
    pub fn system_topic(&self) -> heapless::String<64> {
        let mut topic = heapless::String::new();
        topic.push_str("/system/").ok();
        topic.push_str(self.charger_serial).ok();
        topic
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_config()
    }
}
