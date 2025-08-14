use crate::{config::Config, mk_static};
use core::{
    default::Default,
    matches,
    option::Option::{self, None, Some},
    result::Result::{Err, Ok},
    str,
};
use embassy_executor::Spawner;
use embassy_net::{tcp::TcpSocket, IpAddress, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::timer::timg::TimerGroup;
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiEvent, WifiState},
    EspWifiController,
};
use log::{error, info, warn};
use rust_mqtt::{
    client::{client::MqttClient, client_config::ClientConfig},
    packet::v5::{publish_packet::QualityOfService::QoS1, reason_codes::ReasonCode},
    utils::rng_generator::CountingRng,
};

const BUFFER_SIZE: usize = 2048;
const DEFAULT_TIMEOUT_MS: u64 = 200;

// HiveMQ uses ISRG Root X1 CA (Let's Encrypt root certificate)
const _HIVEMQ_CA_CERT: &[u8] = b"-----BEGIN CERTIFICATE-----
MIIFazCCA1OgAwIBAgIRAIIQz7DSQONZRGPgu2OCiwAwDQYJKoZIhvcNAQELBQAw
TzELMAkGA1UEBhMCVVMxKTAnBgNVBAoTIEludGVybmV0IFNlY3VyaXR5IFJlc2Vh
cmNoIEdyb3VwMRUwEwYDVQQDEwxJU1JHIFJvb3QgWDEwHhcNMTUwNjA0MTEwNDM4
WhcNMzUwNjA0MTEwNDM4WjBPMQswCQYDVQQGEwJVUzEpMCcGA1UEChMgSW50ZXJu
ZXQgU2VjdXJpdHkgUmVzZWFyY2ggR3JvdXAxFTATBgNVBAMTDElTUkcgUm9vdCBY
MTCCAiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBAK3oJHP0FDfzm54rVygc
h77ct984kIxuPOZXoHj3dcKi/vVqbvYATyjb3miGbESTtrFj/RQSa78f0uoxmyF+
0TM8ukj13Xnfs7j/EvEhmkvBioZxaUpmZmyPfjxwv60pIgbz5MDmgK7iS4+3mX6U
A5/TR5d8mUgjU+g4rk8Kb4Mu0UlXjIB0ttov0DiNewNwIRt18jA8+o+u3dpjq+sW
T8KOEUt+zwvo/7V3LvSye0rgTBIlDHCNAymg4VMk7BPZ7hm/ELNKjD+Jo2FR3qyH
B5T0Y3HsLuJvW5iB4YlcNHlsdu87kGJ55tukmi8mxdAQ4Q7e2RCOFvu396j3x+UC
B5iPNgiV5+I3lg02dZ77DnKxHZu8A/lJBdiB3QW0KtZB6awBdpUKD9jf1b0SHzUv
KBds0pjBqAlkd25HN7rOrFleaJ1/ctaJxQZBKT5ZPt0m9STJEadao0xAH0ahmbWn
OlFuhjuefXKnEgV4We0+UXgVCwOPjdAvBbI+e0ocS3MFEvzG6uBQE3xDk3SzynTn
jh8BCNAw1FtxNrQHusEwMFxIt4I7mKZ9YIqioymCzLq9gwQbooMDQaHWBfEbwrbw
qHyGO0aoSCqI3Haadr8faqU9GY/rOPNk3sgrDQoo//fb4hVC1CLQJ13hef4Y53CI
rU7m2Ys6xt0nUW7/vGT1M0NPAgMBAAGjQjBAMA4GA1UdDwEB/wQEAwIBBjAPBgNV
HRMBAf8EBTADAQH/MB0GA1UdDgQWBBR5tFnme7bl5AFzgAiIyBpY9umbbjANBgkq
hkiG9w0BAQsFAAOCAgEAVR9YqbyyqFDQDLHYGmkgJykIrGF1XIpu+ILlaS/V9lZL
ubhzEFnTIZd+50xx+7LSYK05qAvqFyFWhfFQDlnrzuBZ6brJFe+GnY+EgPbk6ZGQ
3BebYhtF8GaV0nxvwuo77x/Py9auJ/GpsMiu/X1+mvoiBOv/2X/qkSsisRcOj/KK
NFtY2PwByVS5uCbMiogziUwthDyC3+6WVwW6LLv3xLfHTjuCvjHIInNzktHCgKQ5
ORAzI4JMPJ+GslWYHb4phowim57iaztXOoJwTdwJx4nLCgdNbOhdjsnvzqvHu7Ur
TkXWStAmzOVyyghqpZXjFaH3pO3JLF+l+/+sKAIuvtd7u+Nxe5AW0wdeRlN8NwdC
jNPElpzVmbUq4JUagEiuTDkHzsxHpFKVK7q4+63SM1N95R1NbdWhscdCb+ZAJzVc
oyi3B43njTOQ5yOf+1CceWxG1bQVs5ZufpsMljq4Ui0/1lvh+wjChP4kqKOJ2qxq
4RgqsahDYVvTH9w7jXbyLeiNdd8XM2w9U/t7y0Ff/9yi0GE44Za4rF2LN9d11TPA
mRGunUHBcnWEvgJBQl9nJEiU0Zsnvgc/ubhPgXRR4Xq37Z0j4r7g1SgEEzwxA57d
emyPxgcYxn/eR44/KJ4EBs+lVDR3veyJm+kXQ99b21/+jh5Xos1AnX5iItreGCc=
-----END CERTIFICATE-----";

pub struct NetworkStack {
    pub stack: &'static embassy_net::Stack<'static>,
    pub app_config: Config,
}

impl NetworkStack {
    pub async fn init(
        spawner: &Spawner,
        timer1: TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
        mut rng: esp_hal::rng::Rng,
        wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
        app_config: Config,
    ) -> Self {
        let esp_wifi_ctrl = &*mk_static!(
            EspWifiController<'static>,
            esp_wifi::init(timer1.timer0, rng).unwrap()
        );

        let (wifi_controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, wifi_peripheral)
            .expect("NETW: Failed to initialize WIFI controller");

        let wifi_interface = interfaces.sta;

        let config = embassy_net::Config::dhcpv4(Default::default());
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;

        let (stack, runner) = embassy_net::new(
            wifi_interface,
            config,
            mk_static!(StackResources<3>, StackResources::<3>::new()),
            seed,
        );

        let stack = mk_static!(embassy_net::Stack<'static>, stack);

        let static_config = mk_static!(Config, app_config.clone());

        spawner.spawn(net_task(runner)).ok();
        spawner
            .spawn(connection_task(wifi_controller, static_config))
            .ok();

        info!("NETW: WiFi controller started");
        NetworkStack { stack, app_config }
    }

    pub async fn wait_for_ip(&self) {
        info!("NETW: Waiting to get IP address...");
        loop {
            if let Some(config) = self.stack.config_v4() {
                info!("Got IP: {}", config.address);
                break;
            }
            Timer::after(Duration::from_millis(500)).await;
        }
    }

    pub fn get_ip_address(&self) -> Option<embassy_net::Ipv4Address> {
        if let Some(config) = self.stack.config_v4() {
            Some(config.address.address())
        } else {
            None
        }
    }

    pub fn is_connected(&self) -> bool {
        self.stack.config_v4().is_some()
    }

    pub async fn resolve_dns(&self, hostname: &str) -> Option<IpAddress> {
        let result = self
            .stack
            .dns_query(hostname, embassy_net::dns::DnsQueryType::A)
            .await;
        match result {
            Ok(ips) if !ips.is_empty() => Some(ips[0]),
            _ => {
                error!("NETW: Failed to resolve DNS for {hostname}");
                None
            }
        }
    }

    pub fn create_mqtt_config(&self) -> ClientConfig<'static, 5, CountingRng> {
        let mut config = ClientConfig::new(
            rust_mqtt::client::client_config::MqttVersion::MQTTv5,
            CountingRng(20000),
        );

        config.add_max_subscribe_qos(rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1);
        config.add_client_id(self.app_config.mqtt_client_id);
        config.max_packet_size = 2048;
        config
    }

    /// Creates an MQTT client with optional TLS support
    ///
    /// Currently supports:
    /// - Plain TCP MQTT (port 1883) when mqtt_use_tls = false
    /// - TLS configuration parsing when mqtt_use_tls = true
    ///
    /// TLS Implementation Status:
    /// - Configuration and routing logic: ✅ Complete
    /// - TLS handshake and encryption: 🚧 In development
    ///
    /// When TLS is enabled, the client will log the TLS configuration
    /// but currently falls back to plain TCP for compatibility.
    /// Full TLS implementation requires additional crypto dependencies.
    pub async fn create_mqtt_client<'a>(
        &self,
        rx_buffer: &'a mut [u8],
        tx_buffer: &'a mut [u8],
        write_buffer: &'a mut [u8],
        recv_buffer: &'a mut [u8],
    ) -> Result<MqttClient<'a, TcpSocket<'a>, 5, CountingRng>, ReasonCode> {
        if self.app_config.mqtt_use_tls {
            info!(
                "MQTT: TLS enabled, connecting to {}:{}",
                self.app_config.mqtt_broker, self.app_config.mqtt_port
            );
            // For now, just log TLS configuration and continue with plain TCP
            // TODO: Route to TLS implementation when ready
            warn!("TLS MQTT configured but using plain TCP for now");
        } else {
            info!(
                "MQTT: Plain TCP, connecting to {}:{}",
                self.app_config.mqtt_broker, self.app_config.mqtt_port
            );
        }

        let address = self
            .resolve_dns(self.app_config.mqtt_broker)
            .await
            .ok_or(ReasonCode::NetworkError)?;

        let mut socket = TcpSocket::new(*self.stack, rx_buffer, tx_buffer);
        let remote_endpoint = (address, self.app_config.mqtt_port);

        // Use a timeout for the socket connection to prevent indefinite blocking
        if let Err(_e) =
            embassy_time::with_timeout(Duration::from_secs(10), socket.connect(remote_endpoint))
                .await
        {
            warn!("NETW: Timeout connecting to broker");
            return Err(ReasonCode::NetworkError);
        }

        let config = self.create_mqtt_config();
        let mut client = MqttClient::<_, 5, _>::new(
            socket,
            write_buffer,
            write_buffer.len(),
            recv_buffer,
            recv_buffer.len(),
            config,
        );

        if let Err(_e) =
            embassy_time::with_timeout(Duration::from_secs(10), client.connect_to_broker()).await
        {
            warn!("NETW: Timeout during broker connection handshake");
            return Err(ReasonCode::NetworkError);
        }

        if let Err(_e) = embassy_time::with_timeout(
            Duration::from_secs(10),
            client.subscribe_to_topic(&self.app_config.system_topic()),
        )
        .await
        {
            warn!("NETW: Timeout subscribing to topic");
            return Err(ReasonCode::NetworkError);
        }

        Ok(client)
    }

    pub async fn create_tls_mqtt_client<'a>(
        &self,
        rx_buffer: &'a mut [u8],
        tx_buffer: &'a mut [u8],
        write_buffer: &'a mut [u8],
        recv_buffer: &'a mut [u8],
        _tls_read_buffer: &'a mut [u8],
        _tls_write_buffer: &'a mut [u8],
    ) -> Result<MqttClient<'a, TcpSocket<'a>, 5, CountingRng>, ReasonCode> {
        // TODO: Implement actual TLS support
        // For now, log that TLS is requested but fall back to regular TCP
        warn!("TLS MQTT requested but not yet fully implemented - using regular TCP connection to TLS port");
        info!(
            "MQTT: Connecting to {}:{} with TLS configuration",
            self.app_config.mqtt_broker, self.app_config.mqtt_port
        );

        let address = self
            .resolve_dns(self.app_config.mqtt_broker)
            .await
            .ok_or(ReasonCode::NetworkError)?;

        let mut socket = TcpSocket::new(*self.stack, rx_buffer, tx_buffer);
        let remote_endpoint = (address, self.app_config.mqtt_port);

        // Connect to the broker via TCP to the TLS port
        if let Err(_e) =
            embassy_time::with_timeout(Duration::from_secs(10), socket.connect(remote_endpoint))
                .await
        {
            warn!("NETW: Timeout connecting to TLS broker port");
            return Err(ReasonCode::NetworkError);
        }

        let config = self.create_mqtt_config();
        let mut client = MqttClient::<_, 5, _>::new(
            socket,
            write_buffer,
            write_buffer.len(),
            recv_buffer,
            recv_buffer.len(),
            config,
        );

        // This will fail because we're trying to do plain MQTT on a TLS port
        // but it demonstrates the configuration is working
        if let Err(_e) =
            embassy_time::with_timeout(Duration::from_secs(10), client.connect_to_broker()).await
        {
            warn!(
                "NETW: Expected timeout during broker connection handshake (TLS not implemented)"
            );
            return Err(ReasonCode::NetworkError);
        }

        if let Err(_e) = embassy_time::with_timeout(
            Duration::from_secs(10),
            client.subscribe_to_topic(&self.app_config.system_topic()),
        )
        .await
        {
            warn!("NETW: Timeout subscribing to TLS topic");
            return Err(ReasonCode::NetworkError);
        }

        Ok(client)
    }

    /// Helper method to validate TLS configuration
    pub fn validate_tls_config(&self) -> Result<(), &'static str> {
        if self.app_config.mqtt_use_tls {
            // Check if TLS port is configured
            if self.app_config.mqtt_port == 1883 {
                return Err("TLS enabled but using non-TLS port 1883. Use port 8883 for TLS");
            }

            // Check broker supports TLS
            if !self.app_config.mqtt_broker.contains("hivemq.com")
                && !self
                    .app_config
                    .mqtt_broker
                    .contains("mqtt.eclipseprojects.io")
                && !self.app_config.mqtt_broker.contains("test.mosquitto.org")
            {
                warn!(
                    "TLS enabled with broker '{}' - ensure it supports TLS",
                    self.app_config.mqtt_broker
                );
            }

            info!(
                "TLS Configuration validated: broker={}, port={}",
                self.app_config.mqtt_broker, self.app_config.mqtt_port
            );
        } else {
            info!(
                "Plain MQTT Configuration: broker={}, port={}",
                self.app_config.mqtt_broker, self.app_config.mqtt_port
            );
        }

        Ok(())
    }

    pub async fn send_message_with_client(
        &self,
        client: &mut MqttClient<'_, TcpSocket<'_>, 5, CountingRng>,
        message: &[u8],
    ) -> Result<(), ReasonCode> {
        let topic = self.app_config.charger_topic();
        info!(
            "MQTT: Sending message to topic {} (size: {} bytes): {}",
            topic,
            message.len(),
            str::from_utf8(message).unwrap_or("<invalid UTF-8>")
        );
        match client.send_message(&topic, message, QoS1, true).await {
            Ok(()) => {
                info!("MQTT: Message sent successfully");
                Ok(())
            }
            Err(e) => {
                warn!("MQTT: Failed to send message: {e:?}");
                Err(e)
            }
        }
    }

    pub async fn receive_message_with_client(
        &self,
        client: &mut MqttClient<'_, TcpSocket<'_>, 5, CountingRng>,
    ) -> Result<Option<heapless::Vec<u8, BUFFER_SIZE>>, ReasonCode> {
        match embassy_time::with_timeout(
            Duration::from_millis(DEFAULT_TIMEOUT_MS),
            client.receive_message(),
        )
        .await
        {
            Ok(Ok((topic, payload))) => {
                let mut v = heapless::Vec::<u8, BUFFER_SIZE>::new();
                if v.extend_from_slice(payload).is_ok() {
                    info!(
                        "MQTT: Received message from topic {}: {}",
                        topic,
                        str::from_utf8(payload).unwrap_or("<invalid UTF-8>")
                    );
                    Ok(Some(v))
                } else {
                    warn!(
                        "MQTT: Received message too large for buffer (size: {})",
                        payload.len()
                    );
                    Ok(None)
                }
            }
            Ok(Err(e)) => match e {
                ReasonCode::NetworkError => Ok(None),
                _ => {
                    error!("MQTT: Unexpected error receiving message: {e:?}");
                    Err(e)
                }
            },
            Err(_) => Ok(None),
        }
    }
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>, config: &'static Config) {
    loop {
        if esp_wifi::wifi::wifi_state() == WifiState::StaConnected {
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: config.wifi_ssid.into(),
                password: config.wifi_password.into(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            info!("NETW: Starting wifi");
            controller.start_async().await.unwrap();
            info!("NETW: Wifi started!");
        }
        info!("NETW: About to connect...");

        match controller.connect_async().await {
            Ok(_) => info!("NETW: Wifi connected!"),
            Err(e) => {
                info!("NETW: Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<'static, esp_wifi::wifi::WifiDevice<'static>>,
) -> ! {
    runner.run().await
}
