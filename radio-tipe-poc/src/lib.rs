pub mod device;
pub mod error;
pub mod socket;

pub enum LoRaDestination {
    Global,
    Group(Vec<LoRaAddress>),
    Unique(LoRaAddress),
}

pub type LoRaAddress = u16;
