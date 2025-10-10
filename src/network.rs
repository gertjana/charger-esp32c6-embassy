use crate::{config::Config, mk_static};
use core::{
    default::Default,
    matches,
    option::Option::{self, None, Some},
    result::Result::{Err, Ok},
    str,
};
use embassy_executor::Spawner;
use embassy_net::{IpAddress, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::timer::timg::TimerGroup;
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiEvent, WifiState},
    EspWifiController,
};
use log::{error, info};

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
                info!("NETW: Got IP: {}", config.address);
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
