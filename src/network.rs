use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embassy_net::StackResources;
use esp_hal::timer::timg::TimerGroup;
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiEvent, WifiState},
    EspWifiController,
};
use core::{
    default::Default, 
    option::Option::{self, Some, None}, 
    result::Result::{Ok, Err},
    matches
};

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

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

        esp_println::println!("WiFi controller started");

        NetworkStack { stack }
    }

    pub async fn wait_for_ip(&self) {
        esp_println::println!("Waiting to get IP address...");
        loop {
            if let Some(config) = self.stack.config_v4() {
                esp_println::println!("Got IP: {}", config.address);
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
            esp_println::println!("Starting wifi");
            controller.start_async().await.unwrap();
            esp_println::println!("Wifi started!");
        }
        esp_println::println!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => esp_println::println!("Wifi connected!"),
            Err(e) => {
                esp_println::println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_wifi::wifi::WifiDevice<'static>>) -> ! {
    runner.run().await
}