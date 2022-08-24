use radio::{Transmit, Power, Receive};
use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use std::fmt::Debug;

pub struct LoRaNetwork<'a, T, C, E> 
where 
    T: Transmit<Error = E> + Receive<Error = E> + Power<Error = E> + radio::Channel<Channel = C, Error = E> + DelayMs<u32> + DelayUs<u32>,
    C: Debug
{
    radio: T,
    signal_channel: Channel<C>,
    transmission_channels: &'a [Channel<C>],
    ingoing_queue: Vec<[u8; 256]>,
    outgoing_queue: Vec<[u8; 256]>,
}

pub struct DelayOptions {
    duty_cycle: f32, 
    min_delay: u64, //us 
    poll_delay: u64, //us
}

pub struct Channel<C> 
where C: Debug
{
    radio_channel: C,
    max_power: u32,
}

