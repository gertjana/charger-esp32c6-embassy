use core::fmt::Write;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use log::info;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

use crate::{charger::ChargerState, config::Config, network::NetworkStack};

/// Display manager for SSD1306 OLED display
pub struct DisplayManager<I2C> {
    display: Ssd1306<
        I2CInterface<I2C>,
        DisplaySize128x64,
        ssd1306::mode::BufferedGraphicsMode<DisplaySize128x64>,
    >,
}

impl<I2C> DisplayManager<I2C>
where
    I2C: embedded_hal::i2c::I2c,
{
    /// Scan I2C bus for devices
    fn scan_i2c_bus(i2c: &mut I2C) -> heapless::Vec<u8, 16> {
        let mut found_devices = heapless::Vec::new();
        info!("Scanning I2C bus for devices...");

        for addr in 0x08..=0x77 {
            // Try to write to each address
            let result = i2c.write(addr, &[]);
            match result {
                Ok(_) => {
                    info!("Found I2C device at address: 0x{addr:02X}");
                    let _ = found_devices.push(addr);
                }
                Err(_) => {
                    // No device at this address, continue scanning
                }
            }
        }

        if found_devices.is_empty() {
            info!("No I2C devices found! Check wiring and pull-up resistors.");
        } else {
            info!("Found {} I2C device(s)", found_devices.len());
        }

        found_devices
    }

    /// Initialize the SSD1306 display
    pub fn new(mut i2c: I2C) -> Result<Self, &'static str> {
        info!("Initializing SSD1306 display...");

        // First, scan the I2C bus to see what devices are available
        let devices = Self::scan_i2c_bus(&mut i2c);

        // Check if we found any devices
        if devices.is_empty() {
            return Err("No I2C devices found - check connections and pull-up resistors");
        }

        // Look for common SSD1306 addresses
        let display_addr = if devices.contains(&0x3C) {
            info!("Found device at 0x3C - typical SSD1306 address");
            0x3C
        } else if devices.contains(&0x3D) {
            info!("Found device at 0x3D - alternative SSD1306 address");
            0x3D
        } else {
            info!("No SSD1306 found at common addresses (0x3C, 0x3D)");
            info!("Available devices: {devices:?}");
            return Err("SSD1306 not found at expected addresses");
        };

        // Try to initialize with the detected address
        info!("Trying I2C address 0x{display_addr:02X}...");
        let interface = I2CDisplayInterface::new_custom_address(i2c, display_addr);

        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        info!("Created display object, attempting init() with address 0x{display_addr:02X}...");

        match display.init() {
            Ok(()) => {
                info!("Display init() completed successfully with address 0x{display_addr:02X}!");
            }
            Err(_) => {
                info!("Failed to initialize display at address 0x{display_addr:02X}");
                return Err("Failed to initialize display - device responded but init failed");
            }
        }

        // Clear the display and flush
        display.flush().map_err(|_| "Failed to flush display")?;
        info!("Display cleared and flushed successfully");

        info!("SSD1306 display initialized successfully");

        Ok(DisplayManager { display })
    }

    /// Update the display with current charger information
    pub fn update_display(
        &mut self,
        config: &Config,
        network: &NetworkStack,
        charger_state: ChargerState,
    ) -> Result<(), &'static str> {
        // Clear the display buffer
        self.display.clear_buffer();

        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();

        // Line 1: Serial number
        let mut serial_line = heapless::String::<21>::new();
        if config.charger_serial.len() > 20 {
            let _ = write!(serial_line, "{}...", &config.charger_serial[..17]);
        } else {
            let _ = write!(serial_line, "{}", config.charger_serial);
        }

        Text::with_baseline(&serial_line, Point::new(0, 0), text_style, Baseline::Top)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw serial")?;

        // Line 2: IP Address
        let mut ip_line = heapless::String::<21>::new();
        if let Some(ip) = network.get_ip_address() {
            let _ = write!(ip_line, "{ip}");
        } else {
            let _ = write!(ip_line, "Not Connected");
        }

        Text::with_baseline(&ip_line, Point::new(0, 12), text_style, Baseline::Top)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw IP address")?;

        // Line 4: Current state
        let mut state_line = heapless::String::<21>::new();
        let _ = write!(state_line, "{}", charger_state.as_str());

        Text::with_baseline(&state_line, Point::new(0, 36), text_style, Baseline::Top)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw state")?;

        // Line 5: Current time (if NTP is synced)
        let mut time_line = heapless::String::<21>::new();
        // Minimum length for ISO8601 format "YYYY-MM-DDTHH:MM:SSZ"
        const ISO8601_MIN_LEN: usize = 19;
        if crate::ntp::is_time_synced() {
            let time_str = crate::ntp::get_iso8601_time();
            // Extract just the time part (HH:MM:SS)
            if time_str.len() >= ISO8601_MIN_LEN {
                let time_part = &time_str[11..19]; // Extract "HH:MM:SS" from "YYYY-MM-DDTHH:MM:SSZ"
                let _ = write!(time_line, "{time_part}");
            } else {
                let _ = write!(time_line, "--:--:--");
            }
        } else {
            let _ = write!(time_line, "Not Synced");
        }

        Text::with_baseline(&time_line, Point::new(0, 48), text_style, Baseline::Top)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw time")?;

        // Flush the buffer to the display
        self.display
            .flush()
            .map_err(|_| "Failed to flush display")?;

        Ok(())
    }

    /// Clear the display
    pub fn clear(&mut self) -> Result<(), &'static str> {
        self.display.clear_buffer();
        self.display
            .flush()
            .map_err(|_| "Failed to flush display")?;
        Ok(())
    }
}
