#[macro_export]
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

use core::fmt::Write;

// Converts a byte slice to a hex string
// The size of the output string is limited by the generic parameter N
// Each byte is represented by two hex characters, so the maximum size is 2 * N
pub fn bytes_to_hex_string<const N: usize>(data: &[u8]) -> heapless::String<N> {
    let mut hex_buf: heapless::String<N> = heapless::String::new();
    for &byte in data {
        if write!(hex_buf, "{byte:02x}").is_err() {
            break;
        }
    }
    hex_buf
}
