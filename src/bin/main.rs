#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp32c6_embassy_charged::{
    charger::{Charger, ChargerInput, ChargerState},
    mk_static,
    network::{self, NetworkStack},
};
use esp_hal::{
    clock::CpuClock,
    gpio::{Input, InputConfig, Level, Output, Pull},
    timer::{systimer::SystemTimer, timg::TimerGroup},
};
use log::info;

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

    info!("Initializing network stack...");
    let network = network::NetworkStack::init(&spawner, timer1, rng, peripherals.WIFI).await;
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
    loop {
        let message = "[2,\"1\",\"Heartbeat\",{}]".as_bytes();
        match network
            .send_mqtt_message("35.159.5.228", "/esp32c6-1/heartbeat", message)
            .await
        {
            Ok(()) => info!("Heartbeat message sent successfully"),
            Err(_) => info!("Failed to send heartbeat message"),
        }

        Timer::after(Duration::from_secs(30)).await;
    }
}
