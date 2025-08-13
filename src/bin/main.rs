#![no_std]
#![no_main]

extern crate alloc;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp32c6_embassy_charged::{
    charger::{self, Charger, ChargerState, InputEvent, OutputEvent},
    config::Config,
    mk_static, mqtt,
    network::{self, NetworkStack},
    ntp, ocpp, utils,
};
use esp_hal::{
    clock::CpuClock,
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    spi::{self, master::Spi},
    time::Rate,
    timer::{systimer::SystemTimer, timg::TimerGroup},
    Blocking,
};

use log::{info, warn};
use mfrc522::{comm::blocking::spi::SpiInterface, Mfrc522};
use rust_mqtt::client::client::MqttClient;
use rust_mqtt::utils::rng_generator::CountingRng;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.5.0

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("MAIN: Charger initialized!");

    let rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    // I2C Setup
    let i2c = I2c::new(peripherals.I2C0, I2cConfig::default())
        .unwrap()
        .into_async()
        .with_sda(peripherals.GPIO22)
        .with_scl(peripherals.GPIO23);

    // Initialize SSD1306 display
    info!("MAIN: Initializing SSD1306 display...");
    let mut display_manager: Option<esp32c6_embassy_charged::display::DisplayManager<_>> =
        match esp32c6_embassy_charged::display::DisplayManager::new(i2c) {
            Ok(mut display) => {
                info!("Display initialized successfully");

                // Draw the startup logo
                match display.draw_logo() {
                    Ok(()) => {
                        info!("MAIN: Logo displayed successfully");
                    }
                    Err(e) => {
                        warn!("MAIN: Failed to draw logo: {e}");
                    }
                }
                Some(display)
            }
            Err(e) => {
                warn!("MAIN: Failed to initialize display: {e}");
                warn!("MAIN: Continuing without display functionality");
                None
            }
        };

    // GPIO Setup
    let onboard_led_pin = Output::new(peripherals.GPIO15, Level::Low, Default::default());

    let cable_lock_pin = Output::new(peripherals.GPIO21, Level::Low, Default::default());

    let cable_switch = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::Up),
    );

    // SPI Cardreader setup
    let spi_bus  = // mk_static!(Spi<Blocking>, 
        Spi::new(
            peripherals.SPI2,
            spi::master::Config::default()
                .with_frequency(Rate::from_mhz(5))
                .with_mode(spi::Mode::_0),
        )
        .unwrap()
        .with_sck(peripherals.GPIO19)
        .with_mosi(peripherals.GPIO18)
        .with_miso(peripherals.GPIO20);
    // );

    let sd_cs = //mk_static!(Output, 
        Output::new(peripherals.GPIO17, Level::High, OutputConfig::default());
    // );

    let charger_relay = Output::new(peripherals.GPIO2, Level::Low, Default::default());

    let charger = mk_static!(Charger, Charger::new());

    match cable_switch.is_low() {
        true => {
            info!("MAIN: Cable is connected, setting initial state to Occupied");
            charger.set_state(ChargerState::Occupied).await;
        }
        false => {
            info!("Cable is not connected, setting initial state to Available");
            charger.set_state(ChargerState::Available).await;
        }
    }

    // Publish initial state to PubSub
    let initial_publisher = charger::STATE_PUBSUB.publisher().unwrap();
    initial_publisher.publish_immediate((ChargerState::Available, heapless::Vec::new()));

    // Load configuration from TOML file with environment variable overrides
    let config = Config::from_config();
    info!(
        "MAIN: Charger configuration loaded: {}",
        config.charger_name
    );

    // Store values we need before config is moved
    let ntp_server = config.ntp_server;

    info!("MAIN: Initializing network stack...");
    let network =
        network::NetworkStack::init(&spawner, timer1, rng, peripherals.WIFI, config).await;
    let network = mk_static!(NetworkStack, network);

    info!("MAIN: Waiting for network connection...");
    network.wait_for_ip().await;
    info!("MAIN: Network connected successfully");

    // Start hardware-related tasks (can run independently of network)
    spawner
        .spawn(charger_led_task(onboard_led_pin, charger))
        .ok();

    spawner.spawn(cable_lock_task(cable_lock_pin)).ok();

    spawner.spawn(charger_cable_task(cable_switch)).ok();

    spawner.spawn(card_swipe_task(spi_bus, sd_cs, charger)).ok();

    spawner.spawn(charger_relay_task(charger_relay)).ok();

    spawner
        .spawn(charger::statemachine_handler_task(charger))
        .ok();

    // Perform initial NTP time synchronization
    info!("MAIN: Synchronizing time with NTP server...");
    let mut sync_attempts = 0;
    let max_sync_attempts = 3;

    while !ntp::is_time_synced() && sync_attempts < max_sync_attempts {
        sync_attempts += 1;
        info!("MAIN: NTP sync attempt {sync_attempts} of {max_sync_attempts}");

        match ntp::sync_time_with_ntp(network, ntp_server).await {
            Ok(()) => {
                info!("MAIN: NTP: Initial time synchronization successful");
                info!("MAIN: NTP: Current time: {}", ntp::get_iso8601_time());
                info!("MAIN: NTP: Timing info: {}", ntp::get_timing_info());
                break;
            }
            Err(e) => {
                warn!("MAIN: NTP: Sync attempt {sync_attempts} failed: {e}");
                if sync_attempts < max_sync_attempts {
                    Timer::after(Duration::from_secs(5)).await;
                }
            }
        }
    }

    if !ntp::is_time_synced() {
        warn!(
            "MAIN: NTP: Failed to synchronize time after {max_sync_attempts} attempts, continuing anyway",
        );
    }

    // Now start network-dependent tasks
    info!("MAIN: Creating MQTT client...");
    let rx_buffer = mk_static!([u8; 2048], [0; 2048]);
    let tx_buffer = mk_static!([u8; 2048], [0; 2048]);
    let write_buffer = mk_static!([u8; 2048], [0; 2048]);
    let recv_buffer = mk_static!([u8; 2048], [0; 2048]);

    match network
        .create_mqtt_client(rx_buffer, tx_buffer, write_buffer, recv_buffer)
        .await
    {
        Ok(client) => {
            info!("MAIN: MQTT client created successfully");
            let client = mk_static!(
                MqttClient<'static, TcpSocket<'static>, 5, CountingRng>,
                client
            );
            spawner.spawn(mqtt::mqtt_client_task(network, client)).ok();

            // Only start NTP sync task after MQTT client is successfully created
            spawner.spawn(ntp::ntp_sync_task(network)).ok();
        }
        Err(e) => {
            warn!("MAIN: Failed to create MQTT client: {e:?}");
            // Could spawn a retry task here if needed
        }
    }

    // Start OCPP-related tasks
    spawner.spawn(ocpp::response_handler_task(charger)).ok();

    spawner.spawn(ocpp::heartbeat_task()).ok();

    spawner.spawn(ocpp::boot_notification_task()).ok();

    spawner.spawn(ocpp::status_notification_task(charger)).ok();

    spawner.spawn(ocpp::authorize_task(charger)).ok();

    spawner.spawn(ocpp::transaction_handler_task(charger)).ok();

    let mut old_state = charger.get_state().await;
    let mut last_display_update = Instant::now();

    info!("MAIN: Starting main loop...");
    loop {
        if let Some(ref mut display) = display_manager {
            if last_display_update.elapsed() >= Duration::from_millis(900) {
                let temp_config = Config::from_config();
                match display.update_display(&temp_config, network, old_state) {
                    Ok(()) => {
                        // Display updated successfully
                    }
                    Err(e) => {
                        warn!("MAIN: Failed to update display: {e}");
                    }
                }
                last_display_update = Instant::now();
            }
        }

        let current_state = charger.get_state().await;
        if current_state != old_state {
            info!("MAIN: Charger state changed: {}", current_state.as_str());
            old_state = current_state;
        }
        Timer::after(Duration::from_millis(100)).await;
    }
}

/// Task to control the charger LED based on the charging state
#[embassy_executor::task]
async fn charger_led_task(mut led_pin: Output<'static>, charger: &'static Charger) {
    info!("TASK: Started Charger Led Charging Indicator");

    let mut subscriber = charger::STATE_PUBSUB.subscriber().unwrap();

    // Set initial LED state based on current charger state
    let initial_state = charger.get_state().await;
    if initial_state == ChargerState::Charging {
        info!("LED : Setting LED high for initial charging state");
        led_pin.set_high();
    } else {
        info!(
            "LED : Setting LED low for initial state: {}",
            initial_state.as_str()
        );
        led_pin.set_low();
    }

    loop {
        // Wait for state changes via PubSub
        if let embassy_sync::pubsub::WaitResult::Message((current_state, _)) =
            subscriber.next_message().await
        {
            match current_state {
                ChargerState::Charging => {
                    info!("LED: Setting LED high for charging state");
                    led_pin.set_low();
                }
                _ => {
                    info!("LED: Setting LED low for state: {}", current_state.as_str());
                    led_pin.set_high();
                }
            }
        }
    }
}

/// Task to detect charger cable connection and disconnection
#[embassy_executor::task]
async fn charger_cable_task(mut button: Input<'static>) {
    info!("TASK: Started Charger cable Detector");

    loop {
        button.wait_for_any_edge().await;

        Timer::after(Duration::from_millis(300)).await; // Debounce delay
        let new_state = button.is_low();

        // Send the appropriate event based on the new state
        let cable_event = if new_state {
            InputEvent::InsertCable
        } else {
            InputEvent::RemoveCable
        };

        info!("CBLE: Detected stable event: {cable_event:?}, sending to state machine");
        charger::STATE_IN_CHANNEL.send(cable_event).await;
    }
}

/// Task to control the charger relay based on the charging state  
#[embassy_executor::task]
async fn charger_relay_task(mut relay: Output<'static>) {
    info!("TASK: Started Charger relay control");

    let mut subscriber = charger::STATE_PUBSUB.subscriber().unwrap();

    relay.set_low();
    info!("RLAY: Initial state set to low (off)");

    loop {
        // Wait for state changes via PubSub
        if let embassy_sync::pubsub::WaitResult::Message((current_state, output_events)) =
            subscriber.next_message().await
        {
            // Simple logic: turn on relay when charging, off otherwise
            match current_state {
                ChargerState::Charging if output_events.contains(&OutputEvent::ApplyPower) => {
                    info!("RLAY: Setting relay high (on)");
                    relay.set_high();
                }
                _ => {
                    info!("RLAY: Setting relay low (off)");
                    relay.set_low();
                }
            }
        }
    }
}

/// Task to control the cable lock based on the charging state
#[embassy_executor::task]
async fn cable_lock_task(mut cable_lock_pin: Output<'static>) {
    info!("TASK: Started Cable Lock Control");
    let mut subscriber = charger::STATE_PUBSUB.subscriber().unwrap();

    loop {
        if let embassy_sync::pubsub::WaitResult::Message((current_state, output_events)) =
            subscriber.next_message().await
        {
            match current_state {
                _ if output_events.contains(&OutputEvent::Lock) => {
                    info!("LOCK: Locking cable for charging state");
                    cable_lock_pin.set_high();
                }
                _ if output_events.contains(&OutputEvent::Unlock) => {
                    info!(
                        "LOCK: Unlocking cable for state: {}",
                        current_state.as_str()
                    );
                    cable_lock_pin.set_low();
                }
                _ => {
                    info!("LOCK: No action for state: {}", current_state.as_str());
                }
            }
        }
    }
}

/// Task to handle card swipe events using the MFRC522 RFID reader
#[embassy_executor::task]
async fn card_swipe_task(
    spi_bus: Spi<'static, Blocking>,
    sd_cs: Output<'static>,
    charger: &'static Charger,
) {
    info!("TASK: Started Card Swipe Detector");

    let delay = Delay::new();
    let spi_dev = ExclusiveDevice::new(spi_bus, sd_cs, delay).unwrap();
    let spi_interface = SpiInterface::new(spi_dev);
    let mut rfid_reader = Mfrc522::new(spi_interface).init().unwrap();

    loop {
        if let Ok(atqa) = rfid_reader.reqa() {
            info!("RFID: Card swipe detected");
            Timer::after(Duration::from_millis(50)).await;
            if let Ok(uid) = rfid_reader.select(&atqa) {
                let hex = utils::bytes_to_hex_string::<24>(uid.as_bytes());
                info!("RFID: UID {hex}");

                charger.set_id_tag(&hex).await;

                charger::STATE_IN_CHANNEL
                    .send(InputEvent::SwipeDetected)
                    .await;
                Timer::after(Duration::from_millis(500)).await;
            }
        }

        Timer::after(Duration::from_secs(1)).await;
    }
}
