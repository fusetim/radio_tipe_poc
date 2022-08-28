use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use radio::{Power, Receive, Transmit, State};
use std::fmt::Debug;
use std::collections::{HashMap, HashSet};

use super::device::{Device, TxClient, RxClient};
use std::marker::PhantomData;
use crate::{LoRaAddress, LoRaDestination};

#[derive(Debug, Copy, Clone)]
pub struct DelayParams {
    duty_cycle: f32,
    min_delay: u64,  //us
    poll_delay: u64, //us
}

#[derive(Debug, Clone)]
pub struct Channel<C>
where
    C: Debug,
{pub
    radio_channel: C,
    delay: DelayParams,
}

type RadioState = radio_sx127x::device::State;

trait Radio<C,E> : Transmit<Error = E>
        + Receive<Error = E>
        + Power<Error = E>
        + radio::Channel<Channel = C, Error = E>
        + State<State=radio_sx127x::device::State, Error=E>
        + DelayMs<u32>
        + DelayUs<u32> {}

impl<C: Debug, E, T: Transmit<Error = E>
        + Receive<Error = E>
        + Power<Error = E>
        + radio::Channel<Channel = C, Error = E>
        + State<State=radio_sx127x::device::State, Error=E>
        + DelayMs<u32>
        + DelayUs<u32>> Radio<C,E> for T {}

#[derive(Debug)]
pub struct LoRaRadio<'a, T, C, E>
where
    T: Radio<C,E>,
    C: Debug,
{
    radio: T,
    signal_channel: Channel<C>,
    transmission_channels: &'a [Channel<C>],
    rx_client: Option<&'a dyn RxClient>,
    tx_client: Option<&'a dyn TxClient>,
    tx_buffer: Vec<LoRaMessage>,
    rx_buffer: Option<(u8, Vec<u8>)>,
    address: LoRaAddress,
    phantom: PhantomData<E>,
}

impl<'a, T, C, E> LoRaRadio<'a, T, C, E>
where
    T: Radio<C,E>,
    C: Debug,
{
    pub fn new(
        radio: T,
        signal_channel: Channel<C>,
        transmission_channels: &'a [Channel<C>],
        rx_client: &'a dyn RxClient,
        tx_client: &'a dyn TxClient,
        address: LoRaAddress,
    ) -> Self {
        Self {
            radio,
            signal_channel,
            transmission_channels,
            rx_client,
            tx_client,
            address,
            tx_buffer: Vec::new(),
            rx_buffer: None,
            phantom: PhantomData,
        }
    }
}

impl<'a, C: Debug, E, T: Radio<C,E>> Device<'a> for LoRaRadio<'a, T,C, E> {
    type DeviceError = RadioError;

    fn set_transmit_client(&mut self, client: &'a dyn TxClient) {
        self.tx_client = client;
    }
    fn set_receive_client(&mut self, client: &'a dyn RxClient) {
        self.rx_client = client;
    }
    fn set_address(&mut self, address: LoRaAddress) {
        self.address = address;
    }
    fn get_address(&self) -> &LoRaAddress {
        self.address
    }
    fn is_transmitting(&mut self) -> Result<bool, Self::DeviceError> {
        match self.radio.get_state() {
            Ok(state) if state == RadioState::Tx || state == RadioState::FsTx => Ok(true),
            Ok(_) => Ok(false),
            // TODO: Use meaningful error 
            Err(err) => Err(RadioError::Unknown{source: format!("{:?}", err)}),
        }
    }
    fn is_receiving(&mut self) -> Result<bool, Self::DeviceError> {
        match self.radio.get_state() {
            Ok(state) if state == RadioState::Rx || state == RadioState::FsRx => Ok(true),
            Ok(_) => Ok(false),
            // TODO: Use meaningful error 
            Err(err) => Err(RadioError::Unknown{source: format!("{:?}", err)}),
        }
    }
    fn queue<'a>(&mut self, dest: LoRaDestination, payload: &'a [u8]) -> Result<(),Self::DeviceError> {
        
    }
    fn transmit(&mut self) {
        unimplemented!()
    }
    fn start_reception(&mut self) {
        unimplemented!()
    }
}

struct LoRaMessage {
    dest: LoRaDestination,
    payload: Vec<u8>,
}

#[derive(thiserror::Error, Debug)]
pub enum RadioError {

    #[error("Outgoing trame queue is full. Device need to transmit queue before adding new payload.")]
    OutgoingQueueFull,

    #[error("Unknown radio error. Context: {}", source)]
    Unknown {
        source: String,
    }
}


