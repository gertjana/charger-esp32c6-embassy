// Embedded-TLS socket implementation for ESP32-C6 with hardware RNG
// Based on embedded-tls v0.17.0 with ESP32-C6 hardware RNG adapter
use log::{info, error, warn};
use embassy_net::tcp::TcpSocket;
use embedded_tls::{TlsConnection, TlsConfig, TlsContext, NoVerify, Aes128GcmSha256};
use crate::esp32_hardware_rng::Esp32HardwareRng;

pub struct EmbeddedTlsSocket<'a> {
    tls_connection: Option<TlsConnection<'a, embassy_net::tcp::TcpSocket<'a>, Aes128GcmSha256>>,
    _phantom: core::marker::PhantomData<&'a ()>,
}

#[derive(Debug)]
pub enum EmbeddedTlsError {
    HandshakeFailed,
    ConnectionClosed,
}

impl<'a> EmbeddedTlsSocket<'a> {
    pub fn new() -> Self {
        Self {
            tls_connection: None,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Connect to TLS endpoint using embedded-tls with ESP32-C6 hardware RNG
    pub async fn connect(
        &mut self,
        stack: &embassy_net::Stack<'static>,
        hostname: &str,
        port: u16,
        tcp_rx_buffer: &'a mut [u8],
        tcp_tx_buffer: &'a mut [u8],
        tls_read_buffer: &'a mut [u8], 
        tls_write_buffer: &'a mut [u8],
        hardware_rng: &mut Esp32HardwareRng,
    ) -> Result<(), embedded_io_async::ErrorKind> {
        info!("TLS: Starting embedded-tls connection with ESP32-C6 hardware RNG to {hostname}:{port}");
        
        // Validate buffer sizes
        if tcp_rx_buffer.len() < 4096 {
            error!("TLS: TCP RX buffer too small, need at least 4096 bytes");
            return Err(embedded_io_async::ErrorKind::InvalidInput);
        }
        if tcp_tx_buffer.len() < 4096 {
            error!("TLS: TCP TX buffer too small, need at least 4096 bytes");
            return Err(embedded_io_async::ErrorKind::InvalidInput);
        }
        if tls_read_buffer.len() < 16640 {
            error!("TLS: TLS read buffer too small, need at least 16640 bytes");
            return Err(embedded_io_async::ErrorKind::InvalidInput);
        }
        if tls_write_buffer.len() < 16640 {
            error!("TLS: TLS write buffer too small, need at least 16640 bytes");
            return Err(embedded_io_async::ErrorKind::InvalidInput);
        }

        // Create TCP socket first
        let mut tcp_socket = TcpSocket::new(*stack, &mut tcp_rx_buffer[..4096], &mut tcp_tx_buffer[..4096]);
        
        // For HiveMQ broker, use DNS resolution instead of hardcoded IP
        info!("TLS: Resolving DNS for hostname: {hostname}");
        
        // Use the network stack's DNS resolution
        let address = match stack.dns_query(hostname, embassy_net::dns::DnsQueryType::A).await {
            Ok(addresses) => {
                if let Some(addr) = addresses.first() {
                    info!("TLS: Resolved {hostname} to {addr:?}");
                    match addr {
                        embassy_net::IpAddress::Ipv4(ipv4) => *ipv4,
                        _ => {
                            error!("TLS: IPv6 not supported for TLS connections");
                            return Err(embedded_io_async::ErrorKind::InvalidInput);
                        }
                    }
                } else {
                    error!("TLS: No IP addresses found for {hostname}");
                    return Err(embedded_io_async::ErrorKind::InvalidInput);
                }
            }
            Err(e) => {
                error!("TLS: DNS resolution failed for {hostname}: {e:?}");
                // Try some common fallback IPs for testing
                if hostname == "mqtt.eclipseprojects.io" {
                    warn!("TLS: Using fallback IP for mqtt.eclipseprojects.io");
                    embassy_net::Ipv4Address::new(198, 41, 30, 241)
                } else if hostname == "broker.emqx.io" {
                    warn!("TLS: Using fallback IP for broker.emqx.io");
                    embassy_net::Ipv4Address::new(3, 136, 154, 110)
                } else if hostname == "test.mosquitto.org" {
                    warn!("TLS: Using fallback IP for test.mosquitto.org");
                    embassy_net::Ipv4Address::new(91, 121, 93, 94)
                } else if hostname == "broker.hivemq.com" {
                    warn!("TLS: Using fallback IP for broker.hivemq.com");
                    embassy_net::Ipv4Address::new(3, 127, 122, 114)
                } else {
                    error!("TLS: No fallback IP available for {hostname}");
                    return Err(embedded_io_async::ErrorKind::InvalidInput);
                }
            }
        };
        
        let remote_endpoint = (address, port);
        
        info!("TLS: Connecting TCP socket to {remote_endpoint:?}");
        
        // Add timeout for TCP connection to prevent hanging
        use embassy_time::{Duration, with_timeout};
        
        match with_timeout(Duration::from_secs(10), tcp_socket.connect(remote_endpoint)).await {
            Ok(Ok(())) => {
                info!("TLS: ✅ TCP connection established successfully to {remote_endpoint:?}");
                info!("TLS: TCP socket state: Connected");
            }
            Ok(Err(e)) => {
                error!("TLS: ❌ TCP connection failed to {remote_endpoint:?}: {e:?}");
                error!("TLS: Check if:");
                error!("TLS:   - The server is reachable on port {}", port);
                error!("TLS:   - The IP address is correct");
                error!("TLS:   - Firewall allows outbound connections on port {}", port);
                return Err(embedded_io_async::ErrorKind::ConnectionRefused);
            }
            Err(_timeout) => {
                error!("TLS: ❌ TCP connection TIMEOUT to {remote_endpoint:?} after 10 seconds");
                error!("TLS: This suggests:");
                error!("TLS:   - Port {} is blocked by firewall", port);
                error!("TLS:   - Server is not responding");
                error!("TLS:   - Network connectivity issues");
                error!("TLS: Try testing with plain TCP port 1883 first");
                return Err(embedded_io_async::ErrorKind::TimedOut);
            }
        }
            
        info!("TLS: TCP connection established, starting TLS handshake");
        
        // Create TLS configuration for embedded-tls with enhanced settings
        info!("TLS: Configuring TLS for server: {}", hostname);
        
        // Try with minimal TLS configuration for better compatibility
        let config = TlsConfig::new()
            .with_server_name(hostname);
            // Note: embedded-tls typically defaults to TLS 1.3 with AES-128-GCM-SHA256
            // Some servers might prefer TLS 1.2 but embedded-tls might not support version selection
        
        info!("TLS: TLS config created, setting up connection buffers");
        info!("TLS: Buffer sizes - read: {}, write: {}", tls_read_buffer.len(), tls_write_buffer.len());
        
        // Create TLS connection with ESP32-C6 hardware RNG
        let mut tls_connection = TlsConnection::new(
            tcp_socket,
            &mut tls_read_buffer[..16640],
            &mut tls_write_buffer[..16640],
        );
        
        // Start TLS handshake with hardware RNG
        info!("TLS: Starting handshake with ESP32-C6 hardware RNG for {}", hostname);
        info!("TLS: Using NoVerify certificate validation for development");
        info!("TLS: Cipher suite: AES-128-GCM-SHA256, TLS version: 1.3 (embedded-tls default)");
        
        let ctx = TlsContext::new(&config, hardware_rng);
        
        // Attempt TLS handshake with better error reporting
        match tls_connection.open::<_, NoVerify>(ctx).await {
            Ok(()) => {
                info!("TLS: ✅ Handshake completed successfully!");
                info!("TLS: Secure connection established to {}", hostname);
            }
            Err(e) => {
                error!("TLS: ❌ Handshake failed with error: {e:?}");
                error!("TLS: This suggests the server doesn't support:");
                error!("TLS:   - TLS 1.3 (embedded-tls default)");
                error!("TLS:   - AES-128-GCM-SHA256 cipher suite");
                error!("TLS:   - Or has other compatibility requirements");
                error!("TLS: Try testing with a broker known to support embedded TLS clients");
                return Err(embedded_io_async::ErrorKind::ConnectionAborted);
            }
        }
            
        info!("TLS: Handshake completed successfully!");
        
        // Store the TLS connection
        self.tls_connection = Some(tls_connection);
        
        Ok(())
    }
}

// Implement embedded_io_async traits for the TLS socket
impl<'a> embedded_io_async::ErrorType for EmbeddedTlsSocket<'a> {
    type Error = embedded_io_async::ErrorKind;
}

impl<'a> embedded_io_async::Read for EmbeddedTlsSocket<'a> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if let Some(ref mut tls_conn) = self.tls_connection {
            tls_conn.read(buf).await
                .map_err(|_| embedded_io_async::ErrorKind::Other)
        } else {
            Err(embedded_io_async::ErrorKind::NotConnected)
        }
    }
}

impl<'a> embedded_io_async::Write for EmbeddedTlsSocket<'a> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if let Some(ref mut tls_conn) = self.tls_connection {
            tls_conn.write(buf).await
                .map_err(|_| embedded_io_async::ErrorKind::Other)
        } else {
            Err(embedded_io_async::ErrorKind::NotConnected)
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        if let Some(ref mut tls_conn) = self.tls_connection {
            tls_conn.flush().await
                .map_err(|_| embedded_io_async::ErrorKind::Other)
        } else {
            Err(embedded_io_async::ErrorKind::NotConnected)
        }
    }
}
