use core::{
    default::Default,
    matches,
    option::Option::{self, None, Some},
    result::Result::{Err, Ok},
    str::FromStr,
};
use crate::mk_static;
use embassy_executor::Spawner;
use embassy_net::{StackResources, tcp::TcpSocket, IpAddress};
use embassy_time::{Duration, Timer};
use esp_hal::timer::timg::TimerGroup;
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiEvent, WifiState},
    EspWifiController,
};
use log::info;
use rust_mqtt::{
    client::{
        client::MqttClient, 
        client_config::ClientConfig
    },
    packet::v5::{
        reason_codes::ReasonCode,
        publish_packet::QualityOfService::QoS1
    },
    utils::rng_generator::CountingRng
};


const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

pub struct NetworkStack {
    pub stack: &'static embassy_net::Stack<'static>,
}

impl NetworkStack {
    pub async fn init(
        spawner: &Spawner,
        timer1: TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
        mut rng: esp_hal::rng::Rng,
        wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    ) -> Self {
        // Initialize WiFi controller
        let esp_wifi_ctrl = &*mk_static!(
            EspWifiController<'static>,
            esp_wifi::init(timer1.timer0, rng.clone()).unwrap()
        );

        let (wifi_controller, interfaces) = esp_wifi::wifi::new(&esp_wifi_ctrl, wifi_peripheral)
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

        // Spawn network tasks
        spawner.spawn(net_task(runner)).ok();
        spawner.spawn(connection_task(wifi_controller)).ok();

        info!("WiFi controller started");
        NetworkStack { stack }
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

    pub fn create_mqtt_config(&self) -> ClientConfig<'static, 5, CountingRng> {
        let mut config = ClientConfig::new(
            rust_mqtt::client::client_config::MqttVersion::MQTTv5,
            CountingRng(20000),
        );
        
        config.add_max_subscribe_qos(rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1);
        config.add_client_id("clientId-8rhWgBODCl");
        config.max_packet_size = 100;

        config
    }

    /// Create and use an MQTT client to send a single message
    /// This is a simplified approach that creates a fresh connection for each message
    pub async fn send_mqtt_message(&self, broker_ip: &str, topic: &str, message: &[u8]) -> Result<(), ReasonCode> {
        let mut rx_buffer = [0; 4096];
        let mut tx_buffer = [0; 4096];
        let mut recv_buffer = [0; 80];
        let mut write_buffer = [0; 80];

        let mut socket = TcpSocket::new(*self.stack, &mut rx_buffer, &mut tx_buffer);
        
        let address = IpAddress::from_str(broker_ip).map_err(|_| ReasonCode::NetworkError)?;
        let remote_endpoint = (address, 1883);
        
        info!("MQTT: Connecting to broker...");
        
        socket.connect(remote_endpoint).await.map_err(|_| ReasonCode::NetworkError)?;
        
        let config = self.create_mqtt_config();
        let mut client = MqttClient::<_, 5, _>::new(socket, &mut write_buffer, 80, &mut recv_buffer, 80, config);

        client.connect_to_broker().await?;
        
        info!("MQTT: Publishing message to topic '{}'", topic);

        match client.send_message(
            topic, 
            message, 
            QoS1,
            true,
        ).await {
            Ok(()) => info!("MQTT: Message sent successfully"),
            Err(_) => info!("MQTT: Failed to send message"),
        };

        Ok(())

    }

}

#[embassy_executor::task]
async fn connection_task(mut controller: WifiController<'static>) {
    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.into(),
                password: PASSWORD.into(),
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
