use chrono::DateTime;
use ocpp_rs::v16::{
    call::{Action, BootNotification, Call, Heartbeat, StartTransaction},
    data_types::DateTimeWrapper,
    parse::Message,
};

use crate::config::Config;

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
    // hack until I get NTP time working
    let datetime = DateTime::from_timestamp_nanos(0);
    Message::Call(Call::new(
        id.into(),
        Action::StartTransaction(StartTransaction {
            connector_id: 0,
            id_tag: id_tag.into(),
            meter_start: 0,
            reservation_id: None,
            timestamp: DateTimeWrapper::new(datetime),
        }),
    ))
}

pub fn stop_transaction(id: &str, transaction_id: i32, id_tag: &str) -> Message {
    // hack until I get NTP time working
    let datetime = DateTime::from_timestamp_nanos(0);
    Message::Call(Call::new(
        id.into(),
        Action::StopTransaction(ocpp_rs::v16::call::StopTransaction {
            transaction_id,
            id_tag: Some(id_tag.into()),
            meter_stop: 0,
            timestamp: DateTimeWrapper::new(datetime),
            reason: None,
            transaction_data: None,
        }),
    ))
}
