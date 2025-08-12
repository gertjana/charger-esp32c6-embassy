use chrono::DateTime;
use core::{
    fmt::Write,
    str::from_utf8,
    sync::atomic::{AtomicU32, Ordering},
};
use embassy_time::{Duration, Timer};
use log::{info, warn};
use ocpp_rs::v16::{
    call::{
        Action, Authorize, BootNotification, Call, Heartbeat, StartTransaction, StatusNotification,
    },
    data_types::DateTimeWrapper,
    enums::{ChargePointErrorCode, ChargePointStatus},
    parse::{self, Message},
};

use crate::{
    charger::{self, Charger, ChargerState, InputEvent, OutputEvent},
    config::Config,
    mqtt::{self},
    ntp, ocpp,
};

/// Thread-safe static counter for OCPP message IDs
static OCPP_MESSAGE_ID_COUNTER: AtomicU32 = AtomicU32::new(1);
pub fn next_ocpp_message_id() -> heapless::String<32> {
    let next = OCPP_MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut data = heapless::String::new();
    let _ = write!(data, "{next}");
    data
}

fn get_timestamp() -> DateTimeWrapper {
    let timestamp = ntp::get_date_time().unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    DateTimeWrapper::new(timestamp)
}

pub fn boot_notification(id: &str, config: &Config) -> Message {
    Message::Call(Call::new(
        id.into(),
        Action::BootNotification(BootNotification {
            charge_point_model: config.charger_model.into(),
            charge_point_vendor: config.charger_vendor.into(),
            firmware_version: Some(env!("CARGO_PKG_VERSION").into()),
            charge_box_serial_number: Some(config.charger_serial.into()),
            charge_point_serial_number: None,
            iccid: None,
            imsi: None,
            meter_serial_number: None,
            meter_type: None,
        }),
    ))
}

pub fn heartbeat(id: &str) -> Message {
    Message::Call(Call::new(id.into(), Action::Heartbeat(Heartbeat {})))
}

pub fn start_transaction(id: &str, id_tag: &str) -> Message {
    Message::Call(Call::new(
        id.into(),
        Action::StartTransaction(StartTransaction {
            connector_id: charger::DEFAULT_CONNECTOR_ID,
            id_tag: id_tag.into(),
            meter_start: 0,
            reservation_id: None,
            timestamp: get_timestamp(),
        }),
    ))
}

pub fn stop_transaction(id: &str, transaction_id: i32, id_tag: &str) -> Message {
    Message::Call(Call::new(
        id.into(),
        Action::StopTransaction(ocpp_rs::v16::call::StopTransaction {
            transaction_id,
            id_tag: Some(id_tag.into()),
            meter_stop: 0,
            timestamp: get_timestamp(),
            reason: None,
            transaction_data: None,
        }),
    ))
}

pub fn status_notification(id: &str, status: ChargerState) -> Message {
    let status = match status {
        ChargerState::Available => ChargePointStatus::Available,
        ChargerState::Occupied => ChargePointStatus::Preparing,
        ChargerState::Charging => ChargePointStatus::Charging,
        ChargerState::Faulted => ChargePointStatus::Faulted,
        ChargerState::Off => ChargePointStatus::Unavailable,
        _ => ChargePointStatus::Unavailable, // Default case
    };
    Message::Call(Call::new(
        id.into(),
        Action::StatusNotification(StatusNotification {
            connector_id: charger::DEFAULT_CONNECTOR_ID,
            error_code: ChargePointErrorCode::NoError,
            status,
            timestamp: Some(get_timestamp()),
            info: None,
            vendor_id: None,
            vendor_error_code: None,
        }),
    ))
}

pub fn authorize(id: &str, id_tag: &str) -> Message {
    Message::Call(Call::new(
        id.into(),
        Action::Authorize(Authorize {
            id_tag: id_tag.into(),
        }),
    ))
}

#[embassy_executor::task]
pub async fn authorize_task() {
    info!("Task started: Authorize Task (PubSub Mode)");

    let config = Config::from_config();
    let mut subscriber = charger::STATE_PUBSUB.subscriber().unwrap();

    loop {
        // Wait for state changes via PubSub
        if let embassy_sync::pubsub::WaitResult::Message((current_state, _)) =
            subscriber.next_message().await
        {
            if current_state == ChargerState::Authorizing {
                info!(
                    "Authorize: Sending authorization request for state: {}",
                    current_state.as_str()
                );
                let authorize_request = authorize(&next_ocpp_message_id(), config.ocpp_id_tag);
                let message = parse::serialize_message(&authorize_request).unwrap();

                match mqtt::MQTT_SEND_CHANNEL
                    .try_send(heapless::Vec::from_slice(message.as_bytes()).unwrap())
                {
                    Ok(()) => {
                        info!("Authorize: Successfully sent authorization request");
                    }
                    Err(_) => {
                        warn!("Authorize: Failed to send authorization request, MQTT queue full");
                    }
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn status_notification_task(charger: &'static Charger) {
    info!("Task started: Status Notification Handler (PubSub Mode)");

    let mut subscriber = charger::STATE_PUBSUB.subscriber().unwrap();

    // Wait to ensure everything is initialized
    Timer::after(Duration::from_secs(3)).await;

    let initial_state = charger.get_state().await;
    info!(
        "Status Notification: Initial state: {}",
        initial_state.as_str()
    );

    // Send initial status notification
    let status_notification =
        ocpp::status_notification(&ocpp::next_ocpp_message_id(), initial_state);
    let message = parse::serialize_message(&status_notification).unwrap();

    match mqtt::MQTT_SEND_CHANNEL.try_send(heapless::Vec::from_slice(message.as_bytes()).unwrap()) {
        Ok(()) => {
            info!(
                "Status Notification: Sent initial OCPP status notification for state: {}",
                initial_state.as_str()
            );
        }
        Err(_) => {
            warn!("Status Notification: Failed to send initial notification, MQTT queue full");
        }
    }

    loop {
        // Wait for state changes via PubSub
        if let embassy_sync::pubsub::WaitResult::Message((current_state, _)) =
            subscriber.next_message().await
        {
            info!(
                "Status Notification: State changed to {}",
                current_state.as_str()
            );

            let status_notification =
                ocpp::status_notification(&ocpp::next_ocpp_message_id(), current_state);
            let message = parse::serialize_message(&status_notification).unwrap();

            if current_state != ChargerState::Authorizing {
                match mqtt::MQTT_SEND_CHANNEL
                    .try_send(heapless::Vec::from_slice(message.as_bytes()).unwrap())
                {
                    Ok(()) => {
                        info!(
                            "Status Notification: Sent OCPP status notification for state: {}",
                            current_state.as_str()
                        );
                    }
                    Err(_) => {
                        warn!("Status Notification: Failed to send notification, MQTT queue full");
                    }
                }
            }
        }
        Timer::after(Duration::from_millis(100)).await; // Avoid busy loop
    }
}

/// Task to send periodic heartbeat messages to the MQTT broker
#[embassy_executor::task]
pub async fn heartbeat_task() {
    info!("Task started: Network Heartbeat");
    Timer::after(Duration::from_secs(5)).await;

    let ocpp_heartbeat_interval = Config::from_config().ocpp_heartbeat_interval;
    loop {
        let heartbeat_req = &ocpp::heartbeat(&ocpp::next_ocpp_message_id());
        let message = parse::serialize_message(heartbeat_req).unwrap();

        let mut msg_vec = heapless::Vec::new();
        if msg_vec.extend_from_slice(message.as_bytes()).is_ok() {
            match mqtt::MQTT_SEND_CHANNEL.try_send(msg_vec) {
                Ok(()) => {
                    info!("Heartbeat: Successfully sent heartbeat message");
                }
                Err(_) => {
                    warn!("Heartbeat: Failed to send heartbeat, MQTT queue full");
                }
            }
        } else {
            warn!("Heartbeat message too large for queue");
        }
        Timer::after(Duration::from_secs(ocpp_heartbeat_interval.into())).await;
    }
}

/// Task to send boot notification to the MQTT broker
/// Note that this task will run only once
#[embassy_executor::task]
pub async fn boot_notification_task() {
    info!("Task started: Boot Notification");

    let boot_notification_req =
        &ocpp::boot_notification(&ocpp::next_ocpp_message_id(), &Config::from_config());
    let message = parse::serialize_message(boot_notification_req).unwrap();

    let mut msg_vec = heapless::Vec::new();
    if msg_vec.extend_from_slice(message.as_bytes()).is_ok() {
        match mqtt::MQTT_SEND_CHANNEL.try_send(msg_vec) {
            Ok(()) => {
                info!("Boot Notification: Successfully sent boot notification");
            }
            Err(_) => {
                warn!("Boot Notification: Failed to send boot notification, MQTT queue full");
            }
        }
    } else {
        warn!("Boot Notification message too large for queue");
    }
}

#[embassy_executor::task]
pub async fn transaction_handler_task(charger: &'static Charger) {
    info!("Task started: OCPP Transaction Handler");

    let config = Config::from_config();
    let mut subscriber = charger::STATE_PUBSUB.subscriber().unwrap();

    loop {
        // Wait for state changes via PubSub
        if let embassy_sync::pubsub::WaitResult::Message((current_state, output_events)) =
            subscriber.next_message().await
        {
            match current_state {
                ChargerState::Charging if output_events.contains(&OutputEvent::ApplyPower) => {
                    let message = parse::serialize_message(&start_transaction(
                        &next_ocpp_message_id(),
                        config.ocpp_id_tag,
                    ))
                    .unwrap();
                    let mut msg_vec = heapless::Vec::new();
                    if msg_vec.extend_from_slice(message.as_bytes()).is_ok() {
                        match mqtt::MQTT_SEND_CHANNEL.try_send(msg_vec) {
                            Ok(()) => {
                                info!("Transaction Handler: Successfully sent StartTransaction message");
                            }
                            Err(_) => {
                                warn!("Transaction Handler: Failed to send StartTransaction message, MQTT queue full");
                            }
                        }
                    }
                }
                ChargerState::Occupied if output_events.contains(&OutputEvent::RemovePower) => {
                    let message = parse::serialize_message(&stop_transaction(
                        &next_ocpp_message_id(),
                        charger.get_transaction_id().await,
                        config.ocpp_id_tag,
                    ))
                    .unwrap(); // TODO: Get transaction ID from state.
                    let mut msg_vec = heapless::Vec::new();
                    if msg_vec.extend_from_slice(message.as_bytes()).is_ok() {
                        match mqtt::MQTT_SEND_CHANNEL.try_send(msg_vec) {
                            Ok(()) => {
                                info!("Transaction Handler: Successfully sent StopTransaction message");
                            }
                            Err(_) => {
                                warn!("Transaction Handler: Failed to send StopTransaction message, MQTT queue full");
                            }
                        }
                    } else {
                        warn!("Transaction Handler: StopTransaction message too large for queue");
                    }
                }
                _ => {
                    // ignoring other states
                }
            }
        }
    }
}

/// Task to handle incoming OCPP responses from MQTT
/// The OCPP library just have proper support for CallResult and CallError
/// so for now we just parse the messages as strings and use string matching
/// to determine the type of message received
/// This is a temporary solution until we have a proper OCPP response handler
/// /// [3,"StartTransaction",{"transactionId":43,"idTagInfo":{"status":"Accepted"}}]
#[embassy_executor::task]
pub async fn response_handler_task(charger: &'static Charger) {
    info!("Task started: OCPP Response Handler");

    loop {
        // Use a timeout when receiving to prevent indefinite blocking
        let message = match embassy_time::with_timeout(
            Duration::from_millis(1000), // 1 second timeout
            mqtt::MQTT_RECEIVE_CHANNEL.receive(),
        )
        .await
        {
            Ok(msg) => msg,
            Err(_) => {
                // Timeout occurred, continue the loop
                Timer::after(Duration::from_millis(10)).await;
                continue;
            }
        };
        let mut new_input_event: InputEvent = InputEvent::None;

        // Convert message bytes to string for parsing
        let message_str = match from_utf8(&message) {
            Ok(s) => s,
            Err(_) => {
                warn!("OCPP: Received invalid UTF-8 message");
                continue;
            }
        };

        // Simple string parsing: [call_result_id, "message_type", {payload}]
        if message_str.starts_with('[') && message_str.ends_with(']') {
            let inner = &message_str[1..message_str.len()-1]; // Remove brackets
            
            // Split into 3 parts: call_result_id, message_type, payload
            let parts: heapless::Vec<&str, 3> = inner.splitn(3, ',').collect();
            
            if parts.len() == 3 {
                // Parse call_result_id
                if let Ok(call_result_id) = parts[0].parse::<u8>() {
                    if call_result_id == 3 { // CallResult
                        // Extract message_type (remove quotes)
                        let message_type = parts[1].trim().trim_matches('"');
                        let payload = parts[2]; // JSON payload as string
                        
                        info!("OCPP: CallResult - Type: {message_type}");
                        
                        match message_type {
                            "Authorize" => {
                                info!("OCPP: Received Authorize response");
                                
                                // Extract status from payload
                                if let Some(status_start) = payload.find("\"status\":\"") {
                                    let status_pos = status_start + 10; // Skip past "status":"
                                    if let Some(status_end) = payload[status_pos..].find('"') {
                                        let status = &payload[status_pos..status_pos + status_end];
                                        if status == "Accepted" {
                                            new_input_event = InputEvent::Accepted;
                                            info!("OCPP: Authorization accepted");
                                        } else {
                                            new_input_event = InputEvent::Rejected;
                                            info!("OCPP: Authorization rejected with status: {status}");
                                        }
                                    }
                                }
                            }
                            "StartTransaction" => {
                                info!("OCPP: Received StartTransaction response");
                                
                                // Extract transaction_id from payload
                                if let Some(tx_start) = payload.find("\"transactionId\":") {
                                    let tx_pos = tx_start + 16; // Skip past "transactionId":
                                    if let Some(tx_end) = payload[tx_pos..].find(&[',', '}'][..]) {
                                        let tx_id_str = &payload[tx_pos..tx_pos + tx_end];
                                        if let Ok(transaction_id) = tx_id_str.parse::<i32>() {
                                            info!("OCPP: Extracted transaction ID: {transaction_id}");
                                            
                                            match embassy_time::with_timeout(
                                                Duration::from_millis(500),
                                                charger.set_transaction_id(transaction_id),
                                            ).await {
                                                Ok(_) => info!("OCPP: Successfully set transaction ID to {transaction_id}"),
                                                Err(_) => warn!("OCPP: Timeout setting transaction ID"),
                                            }
                                        }
                                    }
                                }
                                
                                // Extract status from payload
                                if let Some(status_start) = payload.find("\"status\":\"") {
                                    let status_pos = status_start + 10; // Skip past "status":"
                                    if let Some(status_end) = payload[status_pos..].find('"') {
                                        let status = &payload[status_pos..status_pos + status_end];
                                        if status == "Accepted" {
                                            info!("OCPP: StartTransaction accepted");
                                        } else {
                                            warn!("OCPP: StartTransaction rejected with status: {status}");
                                        }
                                    }
                                }
                            }
                            "Heartbeat" => {
                                info!("OCPP: Received Heartbeat response");
                            }
                            "BootNotification" => {
                                info!("OCPP: Received BootNotification response");
                            }
                            _ => {
                                info!("OCPP: Received other response type: {message_type}");
                            }
                        }
                    } else {
                        info!("OCPP: Non-CallResult message type: {call_result_id}");
                    }
                }
            } else {
                warn!("OCPP: Invalid message format: {message_str}");
            }
        } else {
            info!("MQTT: Non-OCPP message: {message_str}");
        }

        // Send the input event to the state machine if we have one
        if new_input_event != InputEvent::None {
            info!("OCPP: Sending input event to state machine: {new_input_event:?}");
            // Use try_send to avoid blocking indefinitely if the channel is full
            match charger::STATE_IN_CHANNEL.try_send(new_input_event) {
                Ok(_) => info!("OCPP: Successfully sent event to state machine"),
                Err(_) => warn!("OCPP: Failed to send event to state machine, channel full"),
            }
        }
    }
}
