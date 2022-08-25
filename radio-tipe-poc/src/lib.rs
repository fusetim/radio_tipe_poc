use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use radio::{Power, Receive, Transmit};
use std::collections::HashMap;
use std::fmt::Debug;

pub mod error;
pub mod socket;

use error::Result;
use socket::LoRaSocket;

pub enum LoRaDestination<'a> {
    Global,
    Group(&'a [LoRaAddress]),
    Unique(LoRaAddress),
}

pub struct LoRaAddress {
    identifier: u32,
}
