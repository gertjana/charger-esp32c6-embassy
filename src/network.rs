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
        // Initialize WiFi controller
        let esp_wifi_ctrl = &*mk_static!(
            EspWifiController<'static>,
            esp_wifi::init(timer1.timer0, rng).unwrap()
        );

        let (wifi_controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, wifi_peripheral)
            .expect("Failed to initialize WIFI controller");

        let wifi_interface = interfaces.sta;

        let config = embassy_net::Config::dhcpv4(Default::default());
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;

        // Init network stack
        let (stack, runner) = embassy_net::new(
            wifi_interface,
            config,
            mk_static!(StackResources<3>, StackResources::<3>::new()),
            seed,
        );

        // Store stack in static memory
        let stack = mk_static!(embassy_net::Stack<'static>, stack);

        // Store app config in static memory for task access
        let static_config = mk_static!(Config, app_config.clone());

        // Spawn network tasks
        spawner.spawn(net_task(runner)).ok();
        spawner
            .spawn(connection_task(wifi_controller, static_config))
            .ok();

        info!("WiFi controller started");
        NetworkStack { stack, app_config }
    }

    pub async fn wait_for_ip(&self) {
        info!("Waiting to get IP address...");
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
                error!("Failed to resolve DNS for {hostname}");
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

    /// Initialize and return an MQTT client with established connection
    /// This creates buffers, socket, and connects to the MQTT broker
    pub async fn create_mqtt_client<'a>(
        &self,
        rx_buffer: &'a mut [u8],
        tx_buffer: &'a mut [u8],
        write_buffer: &'a mut [u8],
        recv_buffer: &'a mut [u8],
    ) -> Result<MqttClient<'a, TcpSocket<'a>, 5, CountingRng>, ReasonCode> {
        let address = self
            .resolve_dns(self.app_config.mqtt_broker)
            .await
            .ok_or(ReasonCode::NetworkError)?;

        let mut socket = TcpSocket::new(*self.stack, rx_buffer, tx_buffer);
        let remote_endpoint = (address, self.app_config.mqtt_port);

        socket
            .connect(remote_endpoint)
            .await
            .map_err(|_| ReasonCode::NetworkError)?;

        let config = self.create_mqtt_config();
        let mut client = MqttClient::<_, 5, _>::new(
            socket,
            write_buffer,
            write_buffer.len(),
            recv_buffer,
            recv_buffer.len(),
            config,
        );

        client.connect_to_broker().await?;

        client
            .subscribe_to_topic(&self.app_config.system_topic())
            .await?;

        Ok(client)
    }

    pub async fn send_message_with_client(
        &self,
        client: &mut MqttClient<'_, TcpSocket<'_>, 5, CountingRng>,
        message: &[u8],
    ) -> Result<(), ReasonCode> {
        let topic = self.app_config.charger_topic();
        info!(
            "MQTT: Sending message to topic {}: {}",
            topic,
            str::from_utf8(message).unwrap_or("<invalid UTF-8>")
        );
        match client.send_message(&topic, message, QoS1, true).await {
            Ok(()) => info!("MQTT: Message sent successfully"),
            Err(e) => info!("MQTT: Failed to send message: {e:?}"),
        };

        Ok(())
    }

    /// Receive messages from MQTT broker with connection health check
    pub async fn receive_message_with_client(
        &self,
        client: &mut MqttClient<'_, TcpSocket<'_>, 5, CountingRng>,
    ) -> Result<Option<heapless::Vec<u8, BUFFER_SIZE>>, ReasonCode> {
        // Use timeout-based approach to avoid blocking indefinitely
        // This will attempt to receive a message with a short timeout
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
            Ok(Err(e)) => {
                // Only log errors that are not timeout/connection related
                match e {
                    ReasonCode::NetworkError => {
                        // Network errors are common when no message is available
                        // Don't spam logs for this
                        Ok(None)
                    }
                    _ => {
                        error!("MQTT: Unexpected error receiving message: {e:?}");
                        Err(e)
                    }
                }
            }
            Err(_) => {
                // Timeout occurred - no message available, this is normal
                Ok(None)
            }
        }
    }
}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>, config: &'static Config) {
    loop {
        if esp_wifi::wifi::wifi_state() == WifiState::StaConnected {
            // wait until we're no longer connected
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
            info!("Starting wifi");
            controller.start_async().await.unwrap();
            info!("Wifi started!");
        }
        info!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                info!("Failed to connect to wifi: {e:?}");
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
