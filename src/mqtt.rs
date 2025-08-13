use embassy_net::tcp::TcpSocket;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use log::{info, warn};
use rust_mqtt::{client::client::MqttClient, utils::rng_generator::CountingRng};

use crate::network::NetworkStack;

/// Message queues for MQTT messages
pub static MQTT_SEND_CHANNEL: Channel<CriticalSectionRawMutex, heapless::Vec<u8, 2048>, 5> =
    Channel::new();

pub static MQTT_RECEIVE_CHANNEL: Channel<CriticalSectionRawMutex, heapless::Vec<u8, 2048>, 5> =
    Channel::new();

/// Task to handle MQTT client operations
#[embassy_executor::task]
pub async fn mqtt_client_task(
    network: &'static NetworkStack,
    client: &'static mut MqttClient<'static, TcpSocket<'static>, 5, CountingRng>,
) {
    info!("TASK: Started MQTT Client (Send/Receive)");

    loop {
        // Use a timeout to prevent blocking indefinitely
        match embassy_time::with_timeout(
            Duration::from_millis(100),
            network.receive_message_with_client(client),
        )
        .await
        {
            Ok(Ok(Some(message))) => {
                // Use try_send to avoid blocking if the receive channel is full
                if MQTT_RECEIVE_CHANNEL.try_send(message).is_err() {
                    warn!("MQTT: Receive channel is full, dropping message");
                }
            }
            Ok(Ok(None)) => {
                // No message received, continue
            }
            Ok(Err(e)) => {
                warn!("MQTT: Failed to receive MQTT message: {e:?}");
            }
            Err(_) => {
                // Timeout occurred, this is normal when no messages are available
            }
        }

        if let Ok(message) = MQTT_SEND_CHANNEL.try_receive() {
            match network.send_message_with_client(client, &message).await {
                Ok(()) => {
                    // Message sent successfully
                }
                Err(e) => {
                    warn!("MQTT: client task, failed to send message: {e:?}");
                    // Put the message back in the queue to retry later
                    if MQTT_SEND_CHANNEL.try_send(message).is_err() {
                        warn!("MQTT: Failed to requeue message for retry, queue full");
                    }
                }
            }
        }

        Timer::after(Duration::from_millis(50)).await;
    }
}
