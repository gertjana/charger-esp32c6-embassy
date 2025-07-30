#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp32c6_embassy_charged::messages;
use esp32c6_embassy_charged::{
    charger::{Charger, ChargerInput, ChargerState},
    config::Config,
    mk_static,
    network::{self, NetworkStack},
};
use esp_hal::{
    clock::CpuClock,
    gpio::{Input, InputConfig, Level, Output, Pull},
    timer::{systimer::SystemTimer, timg::TimerGroup},
};
use log::{info, warn};
use ocpp_rs::v16::parse::{self};
use rust_mqtt::client::client::MqttClient;
use rust_mqtt::utils::rng_generator::CountingRng;

/// Thread-safe static counter for OCPP message IDs
static OCPP_MESSAGE_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_ocpp_message_id() -> heapless::String<32> {
    let next = OCPP_MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut data = heapless::String::new();
    let _ = write!(data, "{next}");
    data
}

/// Message queues for MQTT messages
static MQTT_SEND_CHANNEL: Channel<CriticalSectionRawMutex, heapless::Vec<u8, 2048>, 5> =
    Channel::new();
static MQTT_RECEIVE_CHANNEL: Channel<CriticalSectionRawMutex, heapless::Vec<u8, 2048>, 5> =
    Channel::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// spawns tasks to:
// - network stack (done)
// - checks card swipes (button) (done)
// - charge cable connect (done)
// - control relay (done)
// - control a display (i2c SSD1306)
// - MQTT client (done)
//    - Send and Receive Queues
// - OCPP Messages

extern crate alloc;

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

    info!("Charger initialized!");

    let rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    // GPIO Setup
    let onboard_led_pin = Output::new(peripherals.GPIO15, Level::Low, Default::default());

    let cable_switch = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::Up),
    );

    let swipe_switch = Input::new(
        peripherals.GPIO1,
        InputConfig::default().with_pull(Pull::Up),
    );

    let charger_relay = Output::new(peripherals.GPIO2, Level::Low, Default::default());

    let charger = mk_static!(Charger, Charger::new());
    charger.set_state(ChargerState::Available).await;

    // Load configuration from TOML file with environment variable overrides
    let config = Config::from_config();
    info!("Charger configuration loaded: {}", config.charger_name);

    info!("Initializing network stack...");
    let network =
        network::NetworkStack::init(&spawner, timer1, rng, peripherals.WIFI, config).await;
    let network = mk_static!(NetworkStack, network);
    network.wait_for_ip().await;

    // Start all the different hardware relatedtasks
    spawner
        .spawn(charger_led_task(charger, onboard_led_pin))
        .ok();
    spawner
        .spawn(charger_cable_task(charger, cable_switch))
        .ok();
    spawner
        .spawn(charger_swipe_task(charger, swipe_switch))
        .ok();
    spawner
        .spawn(charger_relay_task(charger, charger_relay))
        .ok();

    Timer::after(Duration::from_secs(1)).await;

    info!("Creating MQTT client...");
    let rx_buffer = mk_static!([u8; 1024], [0; 1024]);
    let tx_buffer = mk_static!([u8; 1024], [0; 1024]);
    let write_buffer = mk_static!([u8; 2048], [0; 2048]);
    let recv_buffer = mk_static!([u8; 2048], [0; 2048]);

    match network
        .create_mqtt_client(rx_buffer, tx_buffer, write_buffer, recv_buffer)
        .await
    {
        Ok(client) => {
            info!("MQTT client created successfully");
            let client = mk_static!(
                MqttClient<'static, TcpSocket<'static>, 5, CountingRng>,
                client
            );
            spawner.spawn(mqtt_client_task(network, client)).ok();
        }
        Err(e) => {
            warn!("Failed to create MQTT client: {e:?}");
            // Could spawn a retry task here if needed
        }
    }

    spawner.spawn(ocpp_response_handler_task()).ok();

    spawner.spawn(heartbeat_task()).ok();

    spawner.spawn(boot_notification_task()).ok();

    let mut old_state = charger.get_state().await;

    info!("Starting main loop...");
    loop {
        // TODO update display with current state
        let current_state = charger.get_state().await;
        if current_state != old_state {
            info!("Charger state changed: {}", current_state.as_str());
            old_state = current_state;
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Task to handle MQTT client operations
/// This task will continuously check for incoming messages and send outgoing messages
/// from the respective channels.
#[embassy_executor::task]
async fn mqtt_client_task(
    network: &'static NetworkStack,
    client: &'static mut MqttClient<'static, TcpSocket<'static>, 5, CountingRng>,
) {
    info!("Task started: MQTT Client (Send/Receive)");

    loop {
        match network.receive_message_with_client(client).await {
            Ok(Some(message)) => {
                MQTT_RECEIVE_CHANNEL.send(message).await;
            }
            Ok(None) => {
                // No message received, continue to check for outgoing messages
            }
            Err(e) => {
                warn!("Failed to receive MQTT message: {e:?}");
            }
        }

        if let Ok(message) = MQTT_SEND_CHANNEL.try_receive() {
            match network.send_message_with_client(client, &message).await {
                Ok(()) => {
                    // Message sent successfully
                }
                Err(e) => {
                    warn!("Failed to send MQTT message: {e:?}");
                }
            }
        }

        Timer::after(Duration::from_millis(50)).await;
    }
}

/// Task to handle incoming OCPP responses from MQTT
#[embassy_executor::task]
async fn ocpp_response_handler_task() {
    info!("Task started: OCPP Response Handler");

    loop {
        let message = MQTT_RECEIVE_CHANNEL.receive().await;

        match core::str::from_utf8(&message) {
            Ok(message_str) => {
                //TODO Parse the message as an CallResult or CallError
                if message_str.contains("Heartbeat") {
                    info!("OCPP: Received Heartbeat response with timestamp");
                } else if message_str.contains("BootNotification") {
                    info!("OCPP: Received BootNotification response");
                } else if message_str.contains("Authorize") {
                    info!("OCPP: Received Authorize message");
                } else if message_str.contains("StartTransaction") {
                    info!("OCPP: Received StartTransaction message");
                } else if message_str.contains("StopTransaction") {
                    info!("OCPP: Received StopTransaction message");
                } else if message_str.contains("RemoteStartTransaction") {
                    info!("OCPP: Received RemoteStartTransaction command");
                } else if message_str.contains("RemoteStopTransaction") {
                    info!("OCPP: Received RemoteStopTransaction command");
                } else if message_str.contains("StatusNotification") {
                    info!("OCPP: Received StatusNotification message");
                } else if message_str.contains("MeterValues") {
                    info!("OCPP: Received MeterValues message");
                } else if message_str.starts_with('[') && message_str.contains(',') {
                    // Looks like an OCPP message but unknown type
                    info!("OCPP: Received unknown message type: {message_str}");
                } else {
                    // Non-OCPP message
                    info!("MQTT: Non-OCPP message received: {message_str}");
                }
            }
            Err(_) => {
                warn!("MQTT: Received non-UTF8 message, length: {}", message.len());
            }
        }
    }
}

/// Task to control the charger LED based on the charging state
#[embassy_executor::task]
async fn charger_led_task(charger: &'static Charger, mut led_pin: Output<'static>) {
    info!("Task started: Charger Led Charging Indicator");
    loop {
        if (charger.get_state().await).is_charging() {
            led_pin.set_low();
        } else {
            led_pin.set_high();
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Task to detect charger cable connection and disconnection
#[embassy_executor::task]
async fn charger_cable_task(charger: &'static Charger, mut button: Input<'static>) {
    info!("Task started: Charger cable Detector");

    loop {
        button.wait_for_any_edge().await;

        Timer::after(Duration::from_millis(100)).await;

        if button.is_low() {
            charger.transition(ChargerInput::CableConnected).await;
        } else {
            charger.transition(ChargerInput::CableDisconnected).await;
        }
    }
}

/// Task to detect charger swipe events
#[embassy_executor::task]
async fn charger_swipe_task(charger: &'static Charger, mut swipe_switch: Input<'static>) {
    info!("Task started: Charger swipe detector");

    loop {
        swipe_switch.wait_for_falling_edge().await;
        Timer::after(Duration::from_millis(100)).await;

        charger.transition(ChargerInput::SwipeDetected).await;
    }
}

/// Task to control the charger relay based on the charging state
#[embassy_executor::task]
async fn charger_relay_task(charger: &'static Charger, mut relay: Output<'static>) {
    info!("Task started: Charger relay control");

    loop {
        match charger.get_state().await {
            ChargerState::Charging => relay.set_high(),
            _ => relay.set_low(),
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Task to send periodic heartbeat messages to the MQTT broker
#[embassy_executor::task]
async fn heartbeat_task() {
    info!("Task started: Network Heartbeat");
    Timer::after(Duration::from_secs(5)).await;

    loop {
        let heartbeat_req = &messages::heartbeat(&next_ocpp_message_id());
        let message = parse::serialize_message(heartbeat_req).unwrap();

        let mut msg_vec = heapless::Vec::new();
        if msg_vec.extend_from_slice(message.as_bytes()).is_ok() {
            MQTT_SEND_CHANNEL.send(msg_vec).await;
        } else {
            warn!("Heartbeat message too large for queue");
        }
        Timer::after(Duration::from_secs(30)).await;
    }
}

/// Task to send boot notification to the MQTT broker
/// Note that this task will run only once at startup
#[embassy_executor::task]
async fn boot_notification_task() {
    info!("Task started: Boot Notification");

    let boot_notification_req =
        &messages::boot_notification(&next_ocpp_message_id(), &Config::from_config());
    let message = parse::serialize_message(boot_notification_req).unwrap();

    let mut msg_vec = heapless::Vec::new();
    if msg_vec.extend_from_slice(message.as_bytes()).is_ok() {
        MQTT_SEND_CHANNEL.send(msg_vec).await;
    } else {
        warn!("Boot Notification message too large for queue");
    }
}
