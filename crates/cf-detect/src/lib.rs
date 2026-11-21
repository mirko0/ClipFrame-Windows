//! Finding things in captures, all on the CPU and platform-free:
//! - [`codes`]: QR codes and barcodes (rxing),
//! - [`link`]: which decoded texts may be opened as web links,
//! - [`sensitive`]: emails, IP addresses, phone/card numbers and tokens in recognised text.

#![warn(missing_docs)]

pub mod codes;
pub mod link;
pub mod sensitive;
