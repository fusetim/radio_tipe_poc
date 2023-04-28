//! # Radio TIPE PoC
//!
//! This library is the central piece of a TIPE (academic project), and should allow anybody
//! to use this protocol to exchange messages and replicate our results with similar hardware.
//!
//! ## Goals
//! - Provide a real implementation of this protocol that has been proposed.
//! - Provide an implementation that works on embedded devices like the ESP32-DevKitC
//! - Provide a library for application uses. This ensures we have properly structure our network
//!   for real use cases.
//!
//! ## Considerations
//! - This library has only been tested on ESP32-DevKitC and RFM95W modules.
//! - This library relies lightly on `rust-radio-sx127x`, therefore you will need
//!   a LoRa radio based on the SX127x radio.
//! - This library uses the standard library, something that might not be available on most
//!   embedded platforms.
//!
//! ## Caution
//!
//! Please note that this project is an academic/research project and will make
//! some assumptions on the hardware and the actual frames received by the physical
//! radio. DO NOT USE THIS PROJECT FOR REAL USES. It does not enforce any security
//! and will not enforce authenticity neither integrity of the communication.
//!
//! ## Usage
//! Some examples are available at modules [device::device] and [device::radio].

pub mod device;
pub mod error;
pub mod socket;

/// Representation of the recipients for a particular message that will be
/// send or has been received by the LoRa radio.
pub enum LoRaDestination {
    /// This message is for everyone listening.
    ///
    /// Similar to the concept of broadcast in the LAN/WAN world.
    Global,
    /// This message is intended for a group of peers.
    Group(Vec<LoRaAddress>),
    /// This message is intended for a single peer of the network.
    Unique(LoRaAddress),
}

/// Simple alias for the representation of a peer address.
///
/// Some might be more familiar with the similar MAC addresses. Indeed it actually
/// is the physical name of the device and only helps establish link-to-link
/// transmissions.
pub type LoRaAddress = u16;
