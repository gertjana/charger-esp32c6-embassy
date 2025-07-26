use core::cell::RefCell;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use log::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargerState {
    Off,
    Error,
    Available,
    Occupied,
    Charging,
}

impl Default for ChargerState {
    fn default() -> Self {
        Self::Off
    }
}

impl ChargerState {
    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Available | Self::Occupied | Self::Charging)
    }

    pub fn is_charging(&self) -> bool {
        matches!(self, Self::Charging)
    }

    pub fn is_occupied(&self) -> bool {
        matches!(self, Self::Occupied | Self::Charging)
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn has_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Error => "Error",
            Self::Available => "Available",
            Self::Occupied => "Occupied",
            Self::Charging => "Charging",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargerInput {
    CableConnected,
    CableDisconnected,
    SwipeDetected,
}

pub struct Charger {
    state: Mutex<CriticalSectionRawMutex, RefCell<ChargerState>>,
}

impl Default for Charger {
    fn default() -> Self {
        Self::new()
    }
}

impl Charger {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(ChargerState::default())),
        }
    }

    pub async fn get_state(&self) -> ChargerState {
        let state_guard = self.state.lock().await;
        let state = *state_guard.borrow();
        state
    }

    pub async fn set_state(&self, new_state: ChargerState) {
        let state_guard = self.state.lock().await;
        *state_guard.borrow_mut() = new_state;
    }

    pub async fn transition(&self, charger_input: ChargerInput) {
        let state_guard = self.state.lock().await;
        let current_state = *state_guard.borrow();

        let new_state = match (current_state, charger_input) {
            (ChargerState::Occupied, ChargerInput::SwipeDetected) => Some(ChargerState::Charging),
            (ChargerState::Charging, ChargerInput::SwipeDetected) => Some(ChargerState::Occupied),
            (ChargerState::Occupied, ChargerInput::CableDisconnected) => {
                Some(ChargerState::Available)
            }
            (ChargerState::Available, ChargerInput::CableConnected) => Some(ChargerState::Occupied),
            (ChargerState::Charging, ChargerInput::CableDisconnected) => Some(ChargerState::Error),
            (ChargerState::Error, _) => {
                info!("Recovering from error state, resetting in 5 seconds...");
                Timer::after(Duration::from_secs(5)).await;
                Some(ChargerState::Available)
            }
            _ => None,
        };

        if let Some(state) = new_state {
            info!(
                "Transitioned from {} -> {}",
                current_state.as_str(),
                state.as_str()
            );
            *state_guard.borrow_mut() = state;
        } else {
            info!(
                "No valid transition for input: {} with {:?}",
                current_state.as_str(),
                charger_input
            );
        }
    }
}
