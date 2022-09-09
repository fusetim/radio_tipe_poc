use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use radio::{Power, Receive, State, Transmit};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;

use super::device::{Device, QueueError, RxClient, TxClient};
use crate::{LoRaAddress, LoRaDestination};
use std::marker::PhantomData;

use super::frame;
use frame::FrameSize;

const MAX_FRAME_LENGTH: usize = MAX_LORA_PAYLOAD * 5;
const MAX_LORA_PAYLOAD: usize = 253;

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
{
    pub radio_channel: C,
    delay: DelayParams,
}

type RadioState = radio_sx127x::device::State;

pub trait Radio<C, E>:
    Transmit<Error = E>
    + Receive<Error = E>
    + Power<Error = E>
    + radio::Channel<Channel = C, Error = E>
    + State<State = radio_sx127x::device::State, Error = E>
    + DelayMs<u32>
    + DelayUs<u32>
{
}

impl<
        C: Debug,
        E: Debug,
        T: Transmit<Error = E>
            + Receive<Error = E>
            + Power<Error = E>
            + radio::Channel<Channel = C, Error = E>
            + State<State = radio_sx127x::device::State, Error = E>
            + DelayMs<u32>
            + DelayUs<u32>,
    > Radio<C, E> for T
{
}

/// Device implementation for LoRa Radio module.
pub struct LoRaRadio<'a, T, C, E>
where
    T: Radio<C, E>,
    C: Debug,
    E: Debug,
{
    radio: T,
    signal_channel: Channel<C>,
    transmission_channels: &'a [Channel<C>],
    rx_client: Option<&'a dyn RxClient>,
    tx_client: Option<&'a dyn TxClient>,
    tx_buffer: Vec<LoRaMessage>,
    tx_frame: Option<frame::RadioFrameWithHeaders>,
    rx_buffer: Option<(u8, Vec<u8>)>,
    address: LoRaAddress,
    phantom: PhantomData<E>,
}

impl<'a, T, C, E> LoRaRadio<'a, T, C, E>
where
    T: Radio<C, E>,
    C: Debug,
    E: Debug,
{
    /// Initialize a new LoRa radio as device.
    pub fn new(
        radio: T,
        signal_channel: Channel<C>,
        transmission_channels: &'a [Channel<C>],
        rx_client: Option<&'a dyn RxClient>,
        tx_client: Option<&'a dyn TxClient>,
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
            tx_frame: None,
            phantom: PhantomData,
        }
    }

    fn build_trame(
        &self,
        buffer: &Vec<LoRaMessage>,
    ) -> Result<frame::RadioFrameWithHeaders, RadioError> {
        let mut recipients: HashMap<frame::AddressHeader, frame::PayloadFlag> = HashMap::new();
        let mut payloads: Vec<frame::Payload> = Vec::new();
        for (id, msg) in buffer.iter().enumerate() {
            for rec in &msg.dest {
                if let Some(prev) = recipients.get_mut(&(*rec).into()) {
                    prev.push(id as u8);
                } else {
                    recipients.insert((*rec).into(), frame::PayloadFlag::new(&[id as u8]));
                }
            }
            payloads.push(msg.payload.clone());
        }
        match recipients.len() {
            0 => Err(RadioError::InvalidRecipentsError {
                context: format!("No registered recipient!"),
            }),
            1 => {
                let headers = frame::RadioHeaders {
                    rec_n_frames: frame::InfoHeader::new(1, 0),
                    recipients: frame::RecipientHeader::Direct(recipients.iter().map(|(dest, pf)| *dest).next().expect("First recipient does not exist while there is one recipient registered!")),
                    payloads: payloads.len() as u8,
                    sender: self.address.into(),
                };
                let mut frame = frame::RadioFrameWithHeaders { headers, payloads };
                let len = frame.size();
                if len > MAX_FRAME_LENGTH {
                    return Err(RadioError::TooBigFrameError { size: len });
                }
                let frames = (frame.size() / MAX_LORA_PAYLOAD) as u8;
                frame.headers.rec_n_frames.set_frames(frames);
                Ok(frame)
            }
            2..=16 => {
                let headers = frame::RadioHeaders {
                    rec_n_frames: frame::InfoHeader::new(1, 0),
                    recipients: frame::RecipientHeader::Group(recipients.into_iter().collect()),
                    payloads: payloads.len() as u8,
                    sender: self.address.into(),
                };
                let mut frame = frame::RadioFrameWithHeaders { headers, payloads };
                let len = frame.size();
                if len > MAX_FRAME_LENGTH {
                    return Err(RadioError::TooBigFrameError { size: len });
                }
                let frames = (frame.size() / MAX_LORA_PAYLOAD) as u8;
                frame.headers.rec_n_frames.set_frames(frames);
                Ok(frame)
            }
            n => Err(RadioError::InvalidRecipentsError {
                context: format!("Too many recipients ({}, max: 16)!", n),
            }),
        }
    }
}

impl<'a, C: Debug, E: Debug, T: Radio<C, E>> Device<'a> for LoRaRadio<'a, T, C, E> {
    type DeviceError = RadioError;

    fn set_transmit_client(&mut self, client: &'a dyn TxClient) {
        self.tx_client = Some(client);
    }
    fn set_receive_client(&mut self, client: &'a dyn RxClient) {
        self.rx_client = Some(client);
    }
    fn set_address(&mut self, address: LoRaAddress) {
        self.address = address;
    }
    fn get_address(&self) -> LoRaAddress {
        self.address
    }
    fn is_transmitting(&mut self) -> Result<bool, Self::DeviceError> {
        match self.radio.get_state() {
            Ok(state) if state == RadioState::Tx || state == RadioState::FsTx => Ok(true),
            Ok(_) => Ok(false),
            // TODO: Use meaningful error
            Err(err) => Err(RadioError::Unknown {
                context: format!("{:?}", err),
            }),
        }
    }
    fn is_listening(&mut self) -> Result<bool, Self::DeviceError> {
        match self.radio.get_state() {
            Ok(state) if state == RadioState::Rx || state == RadioState::FsRx => Ok(true),
            Ok(_) => Ok(false),
            // TODO: Use meaningful error
            Err(err) => Err(RadioError::Unknown {
                context: format!("{:?}", err),
            }),
        }
    }
    fn queue<'b>(
        &mut self,
        dest: LoRaDestination,
        payload: &'b [u8],
        ack: bool,
    ) -> Result<(), Self::DeviceError> {
        let recipients = match dest {
            LoRaDestination::Global if ack => vec![frame::GLOBAL_ACKNOWLEDGMENT],
            LoRaDestination::Global => vec![frame::GLOBAL_NO_ACKNOWLEDGMENT],
            LoRaDestination::Unique(addr) if ack => {
                vec![(addr & frame::ADDRESS_BITMASK) | frame::ACKNOWLEDGMENT_BITMASK]
            }
            LoRaDestination::Unique(addr) => vec![addr & frame::ADDRESS_BITMASK],
            LoRaDestination::Group(addrs) => addrs
                .into_iter()
                .map(|addr| {
                    if ack {
                        (addr & frame::ADDRESS_BITMASK) | frame::ACKNOWLEDGMENT_BITMASK
                    } else {
                        addr & frame::ADDRESS_BITMASK
                    }
                })
                .collect(),
        };
        if recipients.len() > 48 {
            return Err(QueueError::QueueFullError(
                RadioError::InvalidRecipentsError {
                    context: format!("Too many recipients : {}/48", recipients.len()),
                },
            ));
        }
        let mut buf = self.tx_buffer.clone();
        buf.push(LoRaMessage {
            dest: recipients,
            payload: payload.to_owned(),
        });
        match self.build_trame(&buf) {
            Ok(frame) => {
                self.tx_buffer = buf;
                self.tx_frame = Some(frame);
                Ok(())
            }
            Err(RadioError::TooBigFrameError { size }) => {
                Err(QueueError::QueueFullError(RadioError::TooBigFrameError {
                    size,
                }))
            }
            Err(err) => Err(QueueError::DeviceError(err)),
        }
    }
    fn transmit(&mut self) {
        unimplemented!()
    }
    fn start_reception(&mut self) {
        unimplemented!()
    }
}

/// Internal LoRa Message representation.
#[derive(Debug, Clone)]
struct LoRaMessage {
    dest: Vec<LoRaAddress>,
    payload: Vec<u8>,
}

#[derive(thiserror::Error, Debug)]
pub enum RadioError {
    #[error("Frame is too big to be transmit (is: {}B, max: {}B)!", .size, MAX_FRAME_LENGTH)]
    TooBigFrameError { size: usize },

    #[error("Invalid recipients error, might suggest there is too many or 0 recipients. One recipient might be an invalid address.\nContext: {}", .context)]
    InvalidRecipentsError { context: String },

    #[error("Bad frame error.")]
    FrameError(#[from] frame::FrameError),

    #[error("Unknown radio error. Context: {}", .context)]
    Unknown { context: String },
}
