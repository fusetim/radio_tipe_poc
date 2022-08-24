use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use radio::{Power, Receive, Transmit};
use std::fmt::Debug;
use std::collections::HashMap;

pub mod error;
pub mod socket;

use error::Result;
use socket::LoRaSocket;

/// TODO: Placeholder
type Receiver = u8;

pub struct LoRaNetwork<'a, T, C, E>
where
    T: Transmit<Error = E>
        + Receive<Error = E>
        + Power<Error = E>
        + radio::Channel<Channel = C, Error = E>
        + DelayMs<u32>
        + DelayUs<u32>,
    C: Debug,
{
    radio: T,
    signal_channel: Channel<C>,
    transmission_channels: &'a [Channel<C>],
    ingoing_queue: Vec<[u8; 256]>,
    outgoing_queue: Vec<[u8; 256]>,
    sockets: HashMap<LoRaAddress, Receiver>,
}

impl<'a, T, C, E> LoRaNetwork<'a, T, C, E>
where
    T: Transmit<Error = E>
        + Receive<Error = E>
        + Power<Error = E>
        + radio::Channel<Channel = C, Error = E>
        + DelayMs<u32>
        + DelayUs<u32>,
    C: Debug,
{
    pub fn new(
        radio: T,
        signal_channel: Channel<C>,
        transmission_channels: &'a [Channel<C>],
    ) -> Self {
        Self {
            radio,
            signal_channel,
            transmission_channels,
            ingoing_queue: Vec::new(),
            outgoing_queue: Vec::new(),
            sockets: HashMap::new(),
        }
    }

    pub fn create_socket(&self, dest: LoRaDestination) -> LoRaSocket {
        // TODO
        unimplemented!();
    }

    pub async fn run() -> Result<()> {
        loop {}
    }

    async fn dispatch_outcoming() -> Result<()> {
        Ok(())
    }

    async fn dispatch_incoming() -> Result<()> {
        
        Ok(())
    }
}

pub enum LoRaDestination<'a> {
    Global,
    Group(&'a [LoRaAddress]),
    Unique(LoRaAddress),
}

pub struct LoRaAddress {
    identifier: u32,
}

pub struct DelayParams {
    duty_cycle: f32,
    min_delay: u64,  //us
    poll_delay: u64, //us
}

pub struct Channel<C>
where
    C: Debug,
{
    radio_channel: C,
    delay: DelayParams,
}
