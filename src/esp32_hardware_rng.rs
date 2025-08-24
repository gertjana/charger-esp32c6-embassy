// ESP32-C6 Hardware RNG adapter for embedded-tls
// Implements rand_core::CryptoRngCore using esp_hal::rng::Rng

use esp_hal::rng::Rng;
use rand_core::{CryptoRng, RngCore, Error};

/// ESP32-C6 Hardware RNG adapter for embedded-tls
/// 
/// This adapter bridges the ESP32-C6's hardware RNG (`esp_hal::rng::Rng`) 
/// to the `rand_core::CryptoRngCore` trait required by embedded-tls.
/// 
/// The ESP32-C6 has a True Random Number Generator (TRNG) based on thermal noise
/// which provides cryptographically secure random numbers suitable for TLS.
pub struct Esp32HardwareRng {
    rng: Rng,
}

impl Esp32HardwareRng {
    pub fn new(rng: Rng) -> Self {
        Self { rng }
    }
}

impl RngCore for Esp32HardwareRng {
    fn next_u32(&mut self) -> u32 {
        self.rng.random()
    }

    fn next_u64(&mut self) -> u64 {
        let high = self.rng.random() as u64;
        let low = self.rng.random() as u64;
        (high << 32) | low
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        // ESP32-C6 RNG generates 32-bit values, so we fill in chunks of 4 bytes
        let mut i = 0;
        while i + 4 <= dest.len() {
            let random_u32 = self.rng.random();
            dest[i..i+4].copy_from_slice(&random_u32.to_le_bytes());
            i += 4;
        }
        
        if i < dest.len() {
            let random_u32 = self.rng.random();
            let remaining_bytes = &random_u32.to_le_bytes()[..dest.len() - i];
            dest[i..].copy_from_slice(remaining_bytes);
        }
    }

    /// Try to fill destination buffer with random bytes
    /// For hardware RNG, this should always succeed
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

/// Mark this RNG as cryptographically secure
/// ESP32-C6 hardware RNG is based on thermal noise and is cryptographically secure
impl CryptoRng for Esp32HardwareRng {}

