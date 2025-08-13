use core::fmt::Write;
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_10X20, FONT_6X10},
        MonoTextStyleBuilder,
    },
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyleBuilder},
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
    /// Initialize the SSD1306 display
    pub fn new(i2c: I2C) -> Result<Self, &'static str> {
        info!("DISP:Initializing SSD1306 display...");

        let display_addr = 0x3C;

        // Try to initialize with the detected address
        info!("DISP: Trying I2C address 0x{display_addr:02X}...");
        let interface = I2CDisplayInterface::new_custom_address(i2c, display_addr);

        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        info!("Created display object, attempting init() with address 0x{display_addr:02X}...");

        match display.init() {
            Ok(()) => {
                info!("DISP: Display init() completed successfully with address 0x{display_addr:02X}!");
            }
            Err(_) => {
                info!("DISP: Failed to initialize display at address 0x{display_addr:02X}");
                return Err("Failed to initialize display - device responded but init failed");
            }
        }

        // Clear the display and flush
        display.flush().map_err(|_| "Failed to flush display")?;
        info!("DISP: Display cleared and flushed successfully");

        info!("DISP: SSD1306 display initialized successfully");

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

        // horizontal line
        let stroke_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::On)
            .stroke_width(1)
            .build();

        let left_line = Line::new(
            Point::new(0, 12),   // Start point
            Point::new(128, 12), // End point
        );
        left_line
            .into_styled(stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw left line")?;

        // Line 2: Current state in a full-width rectangle with inverted text and larger font
        let state_text = charger_state.as_str();
        // Using larger FONT_10X20 which is approximately 2x the size of FONT_6X10
        let char_width = 10; // Width per character for FONT_10X20

        // Create a rectangle style for the state background
        let rect_style = PrimitiveStyleBuilder::new()
            .fill_color(BinaryColor::On)
            .stroke_color(BinaryColor::On)
            .stroke_width(1)
            .build();

        // Full width rectangle
        let display_width = 128;

        // Use Rectangle for the full width of the display
        let state_rect = embedded_graphics::primitives::Rectangle::new(
            Point::new(0, 16),            // Starts at left edge, positioned below header line
            Size::new(display_width, 22), // Full width of display, height for the larger font
        )
        .into_styled(rect_style);

        state_rect
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw state background")?;

        // Inverted text style for the state with larger font
        let inverted_text_style = MonoTextStyleBuilder::new()
            .font(&FONT_10X20) // Using the larger font
            .text_color(BinaryColor::Off) // Inverted color
            .build();

        // Calculate the center position for the text
        let text_width = state_text.len() as i32 * char_width;
        let center_x = (display_width as i32 - text_width) / 2;

        // Draw the state text - centered in the rectangle
        Text::with_baseline(
            state_text,
            Point::new(center_x, 16), // Centered horizontally, same vertical position
            inverted_text_style,
            Baseline::Top,
        )
        .draw(&mut self.display)
        .map_err(|_| "Failed to draw state")?;

        // horizontal line0
        let stroke_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::On)
            .stroke_width(1)
            .build();

        let left_line = Line::new(
            Point::new(0, 40),   // Start point
            Point::new(128, 40), // End point
        );
        left_line
            .into_styled(stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw left line")?;

        // Line 4: IP Address
        let mut ip_line = heapless::String::<21>::new();
        if let Some(ip) = network.get_ip_address() {
            let _ = write!(ip_line, "{ip}");
        } else {
            let _ = write!(ip_line, "Not Connected");
        }

        Text::with_baseline(&ip_line, Point::new(0, 46), text_style, Baseline::Top) // Moved down 4 pixels
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw IP address")?;

        // Line 5: Current local time (if NTP is synced)
        let mut time_line = heapless::String::<21>::new();
        if crate::ntp::is_time_synced() {
            let local_time = crate::ntp::get_local_time_formatted(config.timezone_offset_hours);
            let local_date = crate::ntp::get_local_date_formatted(config.timezone_offset_hours);
            let _ = write!(time_line, "{local_date} {local_time}");
        } else {
            let _ = write!(time_line, "Time Not Synced");
        }

        Text::with_baseline(&time_line, Point::new(0, 56), text_style, Baseline::Top) // Moved down 4 pixels
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw time")?;

        // Flush the buffer to the display
        self.display
            .flush()
            .map_err(|_| "Failed to flush display")?;

        Ok(())
    }

    /// Draw the GA Make logo on the display
    pub fn draw_logo(&mut self) -> Result<(), &'static str> {
        // Clear the display buffer first
        self.display.clear_buffer();

        let stroke_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::On)
            .stroke_width(1)
            .build();

        let thick_stroke_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::On)
            .stroke_width(2)
            .build();

        let center_x = 64; // Center of 128px width
        let center_y = 32; // Center of 64px height

        let circle = Circle::new(Point::new(center_x - 25, center_y - 25), 50);
        circle
            .into_styled(thick_stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw main circle")?;

        let left_line = Line::new(
            Point::new(center_x - 15, center_y), // Start point
            Point::new(center_x - 2, center_y),  // End point
        );
        left_line
            .into_styled(stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw left line")?;

        let vertical_down = Line::new(
            Point::new(center_x - 2, center_y),      // Start point
            Point::new(center_x - 2, center_y + 22), // End point (down)
        );
        vertical_down
            .into_styled(stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw vertical down line")?;

        let vertical_up = Line::new(
            Point::new(center_x + 2, center_y + 22), // Start point
            Point::new(center_x + 2, center_y - 22), // End point (up)
        );
        vertical_up
            .into_styled(stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw vertical up line")?;

        let right_line = Line::new(
            Point::new(center_x + 2, center_y),  // Start point
            Point::new(center_x + 20, center_y), // End point
        );
        right_line
            .into_styled(stroke_style)
            .draw(&mut self.display)
            .map_err(|_| "Failed to draw right line")?;

        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();

        Text::with_baseline(
            "Make",
            Point::new(center_x + 20, 55),
            text_style,
            Baseline::Top,
        )
        .draw(&mut self.display)
        .map_err(|_| "Failed to draw logo text")?;

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
