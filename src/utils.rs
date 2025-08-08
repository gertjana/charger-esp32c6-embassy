#[macro_export]
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

use embassy_time::{Duration, Timer};
use esp_hal::gpio::Input;

/// Configuration options for the debounce function
pub struct DebounceConfig {
    /// Duration between readings when verifying a button state
    pub debounce_time: Duration,
    /// Number of consistent readings required to consider a state stable
    pub stable_readings_required: usize,
    /// Duration to wait after processing before accepting new inputs
    pub cooldown_time: Duration,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            debounce_time: Duration::from_millis(50),
            stable_readings_required: 5,
            cooldown_time: Duration::from_millis(10),
        }
    }
}

/// Waits for and debounces a button press or state change.
///
/// There are two main modes of operation:
/// 1. Edge detection (toggle): Detects stable transitions between high and low states
/// 2. Press detection (one-shot): Detects only stable button presses (high-to-low transitions)
///
/// # Arguments
///
/// * `button` - The input button to monitor
/// * `last_stable_state` - For toggle mode: reference to the last known stable state
/// * `one_shot` - If true, only detects button presses (high-to-low); if false, detects any state change
/// * `config` - Configuration options for debouncing behavior
///
/// # Returns
///
/// * `Some(bool)` - If a stable state change is detected, returns the new state
/// * `None` - If no stable state change is detected
pub async fn debounce_input(
    button: &mut Input<'static>,
    last_stable_state: &mut bool,
    one_shot: bool,
    config: &DebounceConfig,
) -> Option<bool> {
    // Wait for any edge
    button.wait_for_any_edge().await;

    let current_state = button.is_low();

    // For one-shot mode, we only care about high-to-low transitions (button presses)
    if one_shot && !current_state {
        return None;
    }

    // For toggle mode, only proceed if the state differs from the last stable state
    if !one_shot && current_state == *last_stable_state {
        return None;
    }

    // Start debouncing process
    let mut stable_count = 0;
    let mut consistent_state = current_state;

    // Debounce algorithm: require multiple consistent readings
    for _ in 0..config.stable_readings_required {
        Timer::after(config.debounce_time).await;

        let new_reading = button.is_low();

        if new_reading == consistent_state {
            stable_count += 1;
        } else {
            // Reset if reading changed
            stable_count = 1;
            consistent_state = new_reading;
        }
    }

    // If we have enough stable readings
    if stable_count == config.stable_readings_required {
        // For toggle mode, update the last stable state
        if !one_shot {
            *last_stable_state = consistent_state;
        }

        // Brief cooldown before accepting new inputs
        Timer::after(config.cooldown_time).await;

        return Some(consistent_state);
    }

    // Not enough stable readings, no state change detected
    Timer::after(config.cooldown_time).await;
    None
}
