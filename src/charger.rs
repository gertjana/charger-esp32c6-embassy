use core::cell::RefCell;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex,
    pubsub::PubSubChannel,
};
use embassy_time::{Duration, Timer};
use log::{info, warn};

pub static DEFAULT_CONNECTOR_ID: u32 = 0;

/// PubSub channel for charger state changes
pub static STATE_PUBSUB: PubSubChannel<
    CriticalSectionRawMutex,
    (ChargerState, heapless::Vec<OutputEvent, 2>),
    8,
    6,
    4,
> = PubSubChannel::new();

/// Message queue for charger input events
pub static STATE_IN_CHANNEL: Channel<CriticalSectionRawMutex, InputEvent, 10> = Channel::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    InsertCable,
    RemoveCable,
    SwipeDetected,
    Accepted,
    Rejected,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputEvent {
    Lock,
    Unlock,
    ApplyPower,
    RemovePower,
    ShowRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargerState {
    Off,
    Faulted,
    Available,
    Occupied,
    Charging,
    Authorizing,
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
        matches!(self, Self::Faulted)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Faulted => "Error",
            Self::Available => "Available",
            Self::Occupied => "Occupied",
            Self::Charging => "Charging",
            Self::Authorizing => "Authorizing",
        }
    }
}

pub struct Charger {
    state: Mutex<CriticalSectionRawMutex, RefCell<ChargerState>>,
    transaction_id: Mutex<CriticalSectionRawMutex, RefCell<i32>>,
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
            transaction_id: Mutex::new(RefCell::new(0)),
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

    pub async fn get_transaction_id(&self) -> i32 {
        let transaction_id_guard = self.transaction_id.lock().await;
        let id = *transaction_id_guard.borrow();
        id
    }

    pub async fn set_transaction_id(&self, new_id: i32) {
        let transaction_id_guard = self.transaction_id.lock().await;
        *transaction_id_guard.borrow_mut() = new_id;
    }

    pub async fn transition(
        &self,
        charger_input: InputEvent,
    ) -> (ChargerState, heapless::Vec<OutputEvent, 2>) {
        let current_state = self.get_state().await;

        info!("Transitioning from {current_state:?} with input {charger_input:?}");

        let (new_state, events) = match (current_state, charger_input) {
            (ChargerState::Available, InputEvent::InsertCable) => {
                (ChargerState::Occupied, heapless::Vec::new())
            }
            (ChargerState::Occupied, InputEvent::SwipeDetected) => {
                (ChargerState::Authorizing, heapless::Vec::new())
            }
            (ChargerState::Authorizing, InputEvent::Accepted) => (
                ChargerState::Charging,
                heapless::Vec::from_slice(&[OutputEvent::ApplyPower, OutputEvent::Lock]).unwrap(),
            ),
            (ChargerState::Authorizing, InputEvent::Rejected) => (
                ChargerState::Occupied,
                heapless::Vec::from_slice(&[OutputEvent::ShowRejected]).unwrap(),
            ),
            (ChargerState::Charging, InputEvent::SwipeDetected) => {
                let output_events =
                    heapless::Vec::from_slice(&[OutputEvent::RemovePower, OutputEvent::Unlock])
                        .unwrap_or_default();
                (ChargerState::Occupied, output_events)
            }
            (ChargerState::Occupied, InputEvent::RemoveCable) => {
                (ChargerState::Available, heapless::Vec::new())
            }
            (ChargerState::Charging, InputEvent::RemoveCable) => {
                let output_events =
                    heapless::Vec::from_slice(&[OutputEvent::RemovePower, OutputEvent::Unlock])
                        .unwrap_or_default();
                (ChargerState::Faulted, output_events)
            }
            (ChargerState::Faulted, _) => {
                warn!("Charger is in faulted state, resetting to available after 5 seconds");
                Timer::after(Duration::from_secs(5)).await;
                STATE_IN_CHANNEL.clear();
                (ChargerState::Available, heapless::Vec::new())
            }
            _ => {
                warn!("Invalid or unknown transition from {current_state:?} with input {charger_input:?}");
                (ChargerState::Faulted, heapless::Vec::new())
            }
        };
        info!("Transition result: {new_state:?}, {events:?}");
        self.set_state(new_state).await;
        (new_state, events)
    }
}

#[embassy_executor::task]
pub async fn statemachine_handler_task(charger: &'static Charger) {
    info!("Task started: Charger State Machine Handler");

    let publisher = STATE_PUBSUB.publisher().unwrap();

    loop {
        // Wait for state change events
        let event = STATE_IN_CHANNEL.receive().await;
        info!("State Machine: Received input event: {event:?}");

        let old_state = charger.get_state().await;
        let (new_state, output_events) = charger.transition(event).await;
        info!(
            "State Machine: Transitioned to state: {}, events: {output_events:?}",
            new_state.as_str()
        );

        // Publish state change if state actually changed
        if old_state != new_state {
            publisher.publish_immediate((new_state, output_events));
            info!(
                "State Machine: Published state change to {}",
                new_state.as_str()
            );
        }

        Timer::after(Duration::from_millis(100)).await;
    }
}
