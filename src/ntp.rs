use chrono::{Datelike, Timelike, Utc};
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_net::udp::UdpSocket;
use embassy_time::{Duration, Instant, Timer};
use log::{error, info, warn};

use crate::config::Config;
use crate::network::NetworkStack;

const NTP_EPOCH_OFFSET: u32 = 2_208_988_800;
const NTP_PACKET_SIZE: usize = 48;
const NTP_PORT: u16 = 123;

static NTP_BASE_TIME: AtomicU32 = AtomicU32::new(0);
static SYSTEM_TIMER_BASE: AtomicU32 = AtomicU32::new(0);
static TIME_SYNCED: AtomicU32 = AtomicU32::new(0);

#[repr(C, packed)]
struct NtpPacket {
    li_vn_mode: u8,       // Leap Indicator, Version Number, Mode
    stratum: u8,          // Stratum level
    poll: u8,             // Poll interval
    precision: i8,        // Clock precision
    root_delay: u32,      // Root delay
    root_dispersion: u32, // Root dispersion
    ref_id: u32,          // Reference identifier
    ref_timestamp: u64,   // Reference timestamp
    orig_timestamp: u64,  // Origin timestamp
    recv_timestamp: u64,  // Receive timestamp
    trans_timestamp: u64, // Transmit timestamp
}

impl NtpPacket {
    fn new_request() -> Self {
        Self {
            li_vn_mode: 0x1B, // Leap indicator: 0, Version: 3, Mode: 3 (client)
            stratum: 0,
            poll: 0,
            precision: 0,
            root_delay: 0,
            root_dispersion: 0,
            ref_id: 0,
            ref_timestamp: 0,
            orig_timestamp: 0,
            recv_timestamp: 0,
            trans_timestamp: 0,
        }
    }

    fn to_bytes(&self) -> [u8; NTP_PACKET_SIZE] {
        let mut bytes = [0u8; NTP_PACKET_SIZE];
        bytes[0] = self.li_vn_mode;
        bytes[1] = self.stratum;
        bytes[2] = self.poll;
        bytes[3] = self.precision as u8;

        // Convert multi-byte fields to network byte order
        bytes[4..8].copy_from_slice(&self.root_delay.to_be_bytes());
        bytes[8..12].copy_from_slice(&self.root_dispersion.to_be_bytes());
        bytes[12..16].copy_from_slice(&self.ref_id.to_be_bytes());
        bytes[16..24].copy_from_slice(&self.ref_timestamp.to_be_bytes());
        bytes[24..32].copy_from_slice(&self.orig_timestamp.to_be_bytes());
        bytes[32..40].copy_from_slice(&self.recv_timestamp.to_be_bytes());
        bytes[40..48].copy_from_slice(&self.trans_timestamp.to_be_bytes());

        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < NTP_PACKET_SIZE {
            return None;
        }

        Some(Self {
            li_vn_mode: bytes[0],
            stratum: bytes[1],
            poll: bytes[2],
            precision: bytes[3] as i8,
            root_delay: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            root_dispersion: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            ref_id: u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            ref_timestamp: u64::from_be_bytes([
                bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22],
                bytes[23],
            ]),
            orig_timestamp: u64::from_be_bytes([
                bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30],
                bytes[31],
            ]),
            recv_timestamp: u64::from_be_bytes([
                bytes[32], bytes[33], bytes[34], bytes[35], bytes[36], bytes[37], bytes[38],
                bytes[39],
            ]),
            trans_timestamp: u64::from_be_bytes([
                bytes[40], bytes[41], bytes[42], bytes[43], bytes[44], bytes[45], bytes[46],
                bytes[47],
            ]),
        })
    }

    fn get_unix_timestamp(&self) -> Option<u32> {
        // Upper 32 bits are seconds, lower 32 bits are fractional seconds
        let ntp_seconds = (self.trans_timestamp >> 32) as u32;

        if ntp_seconds > NTP_EPOCH_OFFSET {
            Some(ntp_seconds - NTP_EPOCH_OFFSET)
        } else {
            None
        }
    }
}

/// Task to synchronize time with NTP servers
#[embassy_executor::task]
pub async fn ntp_sync_task(network: &'static NetworkStack) {
    info!("TASK: Started NTP Time Synchronization");

    Timer::after(Duration::from_secs(60)).await;

    let config = Config::from_config();

    loop {
        if !is_time_synced() || minutes_since_last_sync() > config.ntp_sync_interval_minutes as u32
        {
            info!(
                "NTP : Attempting time synchronization with {}",
                config.ntp_server
            );

            match sync_time_with_ntp(network, config.ntp_server).await {
                Ok(()) => {
                    info!("NTP : Time synchronized successfully");
                    info!("NTP : Current time: {}", get_iso8601_time());
                }
                Err(e) => {
                    warn!("NTP : Time synchronization failed: {e}");
                }
            }

            let wait_time = if is_time_synced() {
                Duration::from_secs(60 * config.ntp_sync_interval_minutes as u64)
            } else {
                Duration::from_secs(900)
            };

            Timer::after(wait_time).await;
        } else {
            Timer::after(Duration::from_secs(1800)).await;
        }
    }
}

pub async fn sync_time_with_ntp(
    stack: &'static NetworkStack,
    server: &str,
) -> Result<(), &'static str> {
    info!("NTP : Starting NTP sync with server: {server}");

    let server_addr = match embassy_time::with_timeout(Duration::from_secs(10), async {
        stack.resolve_dns(server).await
    })
    .await
    {
        Ok(Some(addr)) => addr,
        Ok(None) => return Err("NTP : Failed to resolve NTP server address"),
        Err(_) => return Err("NTP : DNS resolution timeout"),
    };

    let mut rx_meta = heapless::Vec::<embassy_net::udp::PacketMetadata, 2>::new();
    rx_meta
        .resize(2, embassy_net::udp::PacketMetadata::EMPTY)
        .map_err(|_| "NTP : Failed to allocate rx_meta")?;
    let mut rx_buffer = heapless::Vec::<u8, 128>::new();
    rx_buffer
        .resize(128, 0)
        .map_err(|_| "NTP : Failed to allocate rx_buffer")?;
    let mut tx_meta = heapless::Vec::<embassy_net::udp::PacketMetadata, 2>::new();
    tx_meta
        .resize(2, embassy_net::udp::PacketMetadata::EMPTY)
        .map_err(|_| "NTP : Failed to allocate tx_meta")?;
    let mut tx_buffer = heapless::Vec::<u8, 128>::new();
    tx_buffer
        .resize(128, 0)
        .map_err(|_| "NTP : Failed to allocate tx_buffer")?;

    let mut socket = UdpSocket::new(
        *stack.stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );

    if socket.bind(0).is_err() {
        return Err("NTP : Failed to bind UDP socket");
    }

    let request = NtpPacket::new_request();
    let request_bytes = request.to_bytes();

    if socket
        .send_to(&request_bytes, (server_addr, NTP_PORT))
        .await
        .is_err()
    {
        return Err("NTP : Failed to send NTP request");
    }

    info!("NTP : request sent to {server_addr}:{NTP_PORT}");

    let mut response_buffer = [0u8; NTP_PACKET_SIZE];

    let result = match embassy_time::with_timeout(Duration::from_secs(5), async {
        socket.recv_from(&mut response_buffer).await
    })
    .await
    {
        Ok(Ok((len, _addr))) => {
            if len >= NTP_PACKET_SIZE {
                // Parse response
                if let Some(response) = NtpPacket::from_bytes(&response_buffer) {
                    if let Some(unix_timestamp) = response.get_unix_timestamp() {
                        let current_system_time = Instant::now().as_secs() as u32;

                        NTP_BASE_TIME.store(unix_timestamp, Ordering::Relaxed);
                        SYSTEM_TIMER_BASE.store(current_system_time, Ordering::Relaxed);
                        TIME_SYNCED.store(1, Ordering::Relaxed);

                        info!("NTP : sync successful. Unix timestamp: {unix_timestamp}, System time: {current_system_time}s");
                        Ok(())
                    } else {
                        error!("NTP : Invalid timestamp received");
                        Err("Invalid NTP timestamp")
                    }
                } else {
                    error!("NTP : Failed to parse response");
                    Err("Failed to parse NTP response")
                }
            } else {
                error!("NTP : response too short: {len} bytes");
                Err("NTP response too short")
            }
        }
        Ok(Err(_)) => {
            error!("NTP : Socket receive error");
            Err("Socket receive error")
        }
        Err(_) => {
            error!("NTP : request timeout");
            Err("NTP request timeout")
        }
    };

    socket.close();

    result
}

pub fn get_current_unix_time() -> u32 {
    if !is_time_synced() {
        return 0;
    }

    let ntp_base = NTP_BASE_TIME.load(Ordering::Relaxed);
    let system_base = SYSTEM_TIMER_BASE.load(Ordering::Relaxed);
    let current_system_time = Instant::now().as_secs() as u32;

    let elapsed_seconds = current_system_time.wrapping_sub(system_base);

    ntp_base + elapsed_seconds
}

pub fn get_iso8601_time() -> heapless::String<32> {
    let timestamp = get_current_unix_time();

    if timestamp == 0 {
        let mut result = heapless::String::new();
        result.push_str("1970-01-01T00:00:00Z").unwrap();
        return result;
    }

    // Convert Unix timestamp to date and time components
    let mut result = heapless::String::new();

    // Calculate days since Unix epoch
    let days_since_epoch = timestamp / 86400; // 86400 seconds in a day
    let seconds_in_day = timestamp % 86400;

    // Calculate hours, minutes, seconds
    let hours = seconds_in_day / 3600;
    let minutes = (seconds_in_day % 3600) / 60;
    let seconds = seconds_in_day % 60;

    // Calculate year, month, day from days since epoch
    let (year, month, day) = days_to_date(days_since_epoch);

    // Format as ISO8601: YYYY-MM-DDTHH:MM:SSZ
    write_u32_padded(&mut result, year, 4);
    result.push('-').unwrap();
    write_u32_padded(&mut result, month, 2);
    result.push('-').unwrap();
    write_u32_padded(&mut result, day, 2);
    result.push('T').unwrap();
    write_u32_padded(&mut result, hours, 2);
    result.push(':').unwrap();
    write_u32_padded(&mut result, minutes, 2);
    result.push(':').unwrap();
    write_u32_padded(&mut result, seconds, 2);
    result.push('Z').unwrap();

    result
}

pub fn get_date_time() -> Option<chrono::DateTime<Utc>> {
    let timestamp = get_current_unix_time();
    if timestamp == 0 {
        None
    } else {
        chrono::DateTime::<Utc>::from_timestamp(timestamp as i64, 0)
    }
}

/// Check if NTP time synchronization has been completed successfully
pub fn is_time_synced() -> bool {
    TIME_SYNCED.load(Ordering::Relaxed) != 0
}
/// Get the number of minutes since the last NTP sync
pub fn minutes_since_last_sync() -> u32 {
    if !is_time_synced() {
        return u32::MAX; // No sync yet
    }

    let system_base = SYSTEM_TIMER_BASE.load(Ordering::Relaxed);
    let current_system_time = Instant::now().as_secs() as u32;

    let elapsed_seconds = current_system_time.wrapping_sub(system_base);
    elapsed_seconds / 60 // Convert to minutes
}

/// Get detailed timing information for debugging
pub fn get_timing_info() -> heapless::String<128> {
    let mut result = heapless::String::new();

    if is_time_synced() {
        let ntp_base = NTP_BASE_TIME.load(Ordering::Relaxed);
        let system_base = SYSTEM_TIMER_BASE.load(Ordering::Relaxed);
        let current_system_time = Instant::now().as_secs() as u32;
        let elapsed_seconds = current_system_time.wrapping_sub(system_base);
        let current_unix_time = ntp_base + elapsed_seconds;

        write!(
            result,
            "NTP : Synced: {elapsed_seconds}s ago, Unix: {current_unix_time}, Boot: {current_system_time}s",
        ).ok();
    } else {
        write!(result, "Time not synced yet").ok();
    }

    result
}

/// Get local time formatted as a string with timezone offset applied
/// Returns UTC time if no valid timezone offset is provided
pub fn get_local_time_formatted(timezone_offset_hours: i8) -> heapless::String<32> {
    if let Some(utc_datetime) = get_date_time() {
        // Apply timezone offset
        let offset_seconds = timezone_offset_hours as i32 * 3600;
        let local_offset = chrono::FixedOffset::east_opt(offset_seconds)
            .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap()); // Default to UTC if invalid

        let local_datetime = utc_datetime.with_timezone(&local_offset);
        let mut result = heapless::String::new();

        write!(
            result,
            "{:02}:{:02}:{:02}",
            local_datetime.hour(),
            local_datetime.minute(),
            local_datetime.second()
        )
        .ok();

        result
    } else {
        let mut result = heapless::String::new();
        result.push_str("--:--:--").ok();
        result
    }
}

/// Get local date formatted as a string with timezone offset applied
/// Returns UTC date if no valid timezone offset is provided
pub fn get_local_date_formatted(timezone_offset_hours: i8) -> heapless::String<16> {
    if let Some(utc_datetime) = get_date_time() {
        // Apply timezone offset
        let offset_seconds = timezone_offset_hours as i32 * 3600;
        let local_offset = chrono::FixedOffset::east_opt(offset_seconds)
            .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap()); // Default to UTC if invalid

        let local_datetime = utc_datetime.with_timezone(&local_offset);
        let mut result = heapless::String::new();

        write!(
            result,
            "{:02}/{:02}",
            local_datetime.month(),
            local_datetime.day()
        )
        .ok();

        result
    } else {
        let mut result = heapless::String::new();
        result.push_str("--/--").ok();
        result
    }
}

fn write_u32_padded(s: &mut heapless::String<32>, num: u32, width: usize) {
    let mut temp = heapless::String::<12>::new();
    write_u32_to_temp(&mut temp, num);

    // Add leading zeros if needed
    for _ in temp.len()..width {
        s.push('0').unwrap();
    }

    s.push_str(&temp).unwrap();
}

fn write_u32_to_temp(s: &mut heapless::String<12>, mut num: u32) {
    if num == 0 {
        s.push('0').unwrap();
        return;
    }

    let mut digits = [0u8; 10];
    let mut count = 0;

    while num > 0 && count < 10 {
        digits[count] = (num % 10) as u8 + b'0';
        num /= 10;
        count += 1;
    }

    for i in (0..count).rev() {
        if s.push(digits[i] as char).is_err() {
            break;
        }
    }
}

fn days_to_date(mut days: u32) -> (u32, u32, u32) {
    // Start from 1970
    let mut year = 1970;

    // Handle full years
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    // Days in each month (non-leap year)
    const DAYS_IN_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let mut month = 1;
    for &days_in_month in &DAYS_IN_MONTH {
        let actual_days = if month == 2 && is_leap_year(year) {
            29 // February in leap year
        } else {
            days_in_month
        };

        if days >= actual_days {
            days -= actual_days;
            month += 1;
        } else {
            break;
        }
    }

    let day = days + 1; // Day is 1-indexed

    (year, month, day)
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
