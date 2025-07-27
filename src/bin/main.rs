#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use alloc::string::ToString;
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
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
use log::info;
use ocpp_rs::v16::{
    call::{Action, Call, Heartbeat},
    parse::{self, Message},
};

/// Thread-safe static counter for OCPP message IDs
static OCPP_MESSAGE_ID_COUNTER: AtomicU32 = AtomicU32::new(1);
fn next_ocpp_message_id() -> heapless::String<32> {
    let next = OCPP_MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut data = heapless::String::new();
    let _ = write!(data, "{next}");
    data
}

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

    // Start the network related tasks

    spawner.spawn(heartbeat_task(network)).ok();

    let mut old_state = charger.get_state().await;
    loop {
        let current_state = charger.get_state().await;
        if current_state != old_state {
            info!("Charger state changed: {}", current_state.as_str());
            old_state = current_state;
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}

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

#[embassy_executor::task]
async fn charger_swipe_task(charger: &'static Charger, mut swipe_switch: Input<'static>) {
    info!("Task started: Charger swipe detector");

    loop {
        swipe_switch.wait_for_falling_edge().await;
        Timer::after(Duration::from_millis(100)).await;

        charger.transition(ChargerInput::SwipeDetected).await;
    }
}

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

#[embassy_executor::task]
async fn heartbeat_task(network: &'static NetworkStack) {
    info!("Task started: Network Heartbeat");
    let broker_ip = network
        .resolve_dns(network.app_config.mqtt_broker)
        .await
        .unwrap();
    let topic = network.app_config.charger_topic();

    loop {
        // Create OCPP 1.6 Heartbeat Call message using thread-safe message ID
        let heartbeat_call = Message::Call(Call::new(
            next_ocpp_message_id().to_string(),
            Action::Heartbeat(Heartbeat {}),
        ));

        let message = parse::serialize_message(&heartbeat_call).unwrap();

        match network
            .send_mqtt_message(&broker_ip.to_string(), topic.as_str(), message.as_bytes())
            .await
        {
            Ok(()) => info!(
                "OCPP Heartbeat message sent successfully to {}",
                topic.as_str()
            ),
            Err(_) => info!("Failed to send OCPP Heartbeat message"),
        }

        Timer::after(Duration::from_secs(30)).await;
    }
}
