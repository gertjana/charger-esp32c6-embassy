use core::str;
use embassy_net::tcp::TcpSocket;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use log::{error, info, warn};
use rust_mqtt::{
    client::{client::MqttClient, client_config::ClientConfig},
    packet::v5::{publish_packet::QualityOfService::QoS1, reason_codes::ReasonCode},
    utils::rng_generator::CountingRng,
};

use crate::{config::Config, network::NetworkStack};

const BUFFER_SIZE: usize = 2048;
const DEFAULT_TIMEOUT_MS: u64 = 100;

/// Message queues for MQTT messages
pub static MQTT_SEND_CHANNEL: Channel<CriticalSectionRawMutex, heapless::Vec<u8, BUFFER_SIZE>, 5> =
    Channel::new();

pub static MQTT_RECEIVE_CHANNEL: Channel<
    CriticalSectionRawMutex,
    heapless::Vec<u8, BUFFER_SIZE>,
    5,
> = Channel::new();

/// Create MQTT configuration for the given app config
pub fn create_mqtt_config(app_config: &Config) -> ClientConfig<'static, 5, CountingRng> {
    let mut config = ClientConfig::new(
        rust_mqtt::client::client_config::MqttVersion::MQTTv5,
        CountingRng(20000),
    );

    config.add_max_subscribe_qos(rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1);
    config.add_client_id(app_config.mqtt_client_id);
    config.max_packet_size = BUFFER_SIZE as u32;
    config
}

/// Create a new MQTT client with the provided buffers and network stack
pub async fn create_mqtt_client<'a>(
    network: &NetworkStack,
    rx_buffer: &'a mut [u8],
    tx_buffer: &'a mut [u8],
    write_buffer: &'a mut [u8],
    recv_buffer: &'a mut [u8],
) -> Result<MqttClient<'a, TcpSocket<'a>, 5, CountingRng>, ReasonCode> {
    let address = network
        .resolve_dns(network.app_config.mqtt_broker)
        .await
        .ok_or(ReasonCode::NetworkError)?;

    let mut socket = TcpSocket::new(*network.stack, rx_buffer, tx_buffer);
    let remote_endpoint = (address, network.app_config.mqtt_port);

    // Use a timeout for the socket connection to prevent indefinite blocking
    if let Err(_e) =
        embassy_time::with_timeout(Duration::from_secs(10), socket.connect(remote_endpoint)).await
    {
        warn!("MQTT: Timeout connecting to broker");
        return Err(ReasonCode::NetworkError);
    }

    let config = create_mqtt_config(&network.app_config);
    let mut client = MqttClient::<_, 5, _>::new(
        socket,
        write_buffer,
        write_buffer.len(),
        recv_buffer,
        recv_buffer.len(),
        config,
    );

    if let Err(_e) =
        embassy_time::with_timeout(Duration::from_secs(10), client.connect_to_broker()).await
    {
        warn!("MQTT: Timeout during broker connection handshake");
        return Err(ReasonCode::NetworkError);
    }

    if let Err(_e) = embassy_time::with_timeout(
        Duration::from_secs(10),
        client.subscribe_to_topic(&network.app_config.system_topic()),
    )
    .await
    {
        warn!("MQTT: Timeout subscribing to topic");
        return Err(ReasonCode::NetworkError);
    }

    Ok(client)
}

/// Send a message using the provided MQTT client
pub async fn send_message_with_client(
    app_config: &Config,
    client: &mut MqttClient<'_, TcpSocket<'_>, 5, CountingRng>,
    message: &[u8],
) -> Result<(), ReasonCode> {
    let topic = app_config.charger_topic();
    info!(
        "MQTT: Sending message to topic {} (size: {} bytes): {}",
        topic,
        message.len(),
        str::from_utf8(message).unwrap_or("<invalid UTF-8>")
    );
    match client.send_message(&topic, message, QoS1, true).await {
        Ok(()) => {
            info!("MQTT: Message sent successfully");
            Ok(())
        }
        Err(e) => {
            warn!("MQTT: Failed to send message: {e:?}");
            Err(e)
        }
    }
}

/// Receive a message using the provided MQTT client
pub async fn receive_message_with_client(
    client: &mut MqttClient<'_, TcpSocket<'_>, 5, CountingRng>,
) -> Result<Option<heapless::Vec<u8, BUFFER_SIZE>>, ReasonCode> {
    match embassy_time::with_timeout(
        Duration::from_millis(DEFAULT_TIMEOUT_MS),
        client.receive_message(),
    )
    .await
    {
        Ok(Ok((topic, payload))) => {
            let mut v = heapless::Vec::<u8, BUFFER_SIZE>::new();
            if v.extend_from_slice(payload).is_ok() {
                info!(
                    "MQTT: Received message from topic {}: {}",
                    topic,
                    str::from_utf8(payload).unwrap_or("<invalid UTF-8>")
                );
                Ok(Some(v))
            } else {
                warn!(
                    "MQTT: Received message too large for buffer (size: {})",
                    payload.len()
                );
                Ok(None)
            }
        }
        Ok(Err(e)) => match e {
            ReasonCode::NetworkError => Ok(None),
            _ => {
                error!("MQTT: Unexpected error receiving message: {e:?}");
                Err(e)
            }
        },
        Err(_) => Ok(None),
    }
}

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
            Duration::from_millis(DEFAULT_TIMEOUT_MS),
            receive_message_with_client(client),
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
            match send_message_with_client(&network.app_config, client, &message).await {
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

        Timer::after(Duration::from_millis(DEFAULT_TIMEOUT_MS)).await;
    }
}
