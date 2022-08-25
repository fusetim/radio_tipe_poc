use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use radio::{Power, Receive, Transmit};
use std::fmt::Debug;
use std::collections::HashMap;

use device::{Device, TxClient, RxClient};
use crate::LoRaAddress;

#[derive(Debug, Copy)]
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

type RadioState = radio-sx127x::device::State;

trait Radio<C,E>: Transmit<Error = E>
        + Receive<Error = E>
        + Power<Error = E>
        + radio::Channel<Channel = C, Error = E>
        + State<State=radio-sx127x::device::State, Error=E>
        + DelayMs<u32>
        + DelayUs<u32>;

#[derive(Debug)]
pub struct LoRaRadio<'a, T, C, E>
where
    T: Radio<C,E>,
    C: Debug,
{
    radio: T,
    signal_channel: Channel<C>,
    transmission_channels: &'a [Channel<C>],
    rx_client: Option<&'a RxClient>,
    tx_client: Option<&'a TxClient>,
    address: LoRaAddress,
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
        rx_client: &'a RxClient,
        tx_client: &'a TxClient,
    ) -> Self {
        Self {
            radio,
            signal_channel,
            transmission_channels,
            rx_client,
            tx_client,
            address,
        }
    }
}

impl Device for LoRaRadio {
   type DeviceError = RadioError;

    fn set_transmit_client(&mut self, client: &'a dyn TxClient) {
        self.tx_client = client;
    }
    fn set_receive_client(&mut self, client: &'a dyn RxClient) {
        self.rx_client = client;
    }
    fn set_address(&mut self, address: LoRaAddress) {
        self.address = address;
    };
    fn get_address(&self) -> &LoRaAddress {
        self.address
    }
    fn is_transmitting(&mut self) -> Result<bool, Self::DeviceError> {
        match self.radio.get_state() {
            Ok(state) if state == RadioState::Tx || state == RadioState::FsTx => Ok(true),
            Ok(_) => Ok(false)
            // TODO: Use meaningful error 
            Err(err) => Err(err) => Err(RadioError::Unknown{source: format!("{:?}", err)}),
        }
    }
    fn is_receiving(&mut self) -> Result<bool, Self::DeviceError> {
        match self.radio.get_state() {
            Ok(state) if state == RadioState::Rx || state == RadioState::FsRx => Ok(true),
            Ok(_) => Ok(false)
            // TODO: Use meaningful error 
            Err(err) => Err(RadioError::Unknown{source: format!("{:?}", err)}),
        }
    }
    fn transmit() {
        unimplemented!()
    }
    fn start_reception() {
        unimplemented!()
    }
}

#[derive(thiserror::Error)]
pub enum RadioError {

    #[error("Unknown radio error. Context: {}", source)]
    Unknown (
        source: String,
    )
}