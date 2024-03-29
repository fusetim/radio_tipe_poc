//! The radio device implementation for a SX127x radio.
//!
//! This is the peripheral you will need to build using [LoRaRadio::new]
//! and initialized in order to use the protocol. Most of the time, you
//! will be able to rely only on the [Device] implementation to operate
//! the radio and exchange frames on the network.
//!
//! ## Usages
//!
//! We will assume you successfully initialized a LoRa radio provided by [radio-sx127x](crate::radio-sx127x).
//! In any case, it will likely depends on the platform you used but it should look similar to:
//! ```rust,ignore
//! let peripherals = Peripherals::take().unwrap();
//! let pins = peripherals.pins;
//!
//! let driver = SpiDriver::new::<SPI2>(peripherals.spi2, pins.gpio5, pins.gpio18, Some(pins.gpio19), Dma::Disabled)?;
//! let config = SpiConfig::new().baudrate(LORA_SPI_FREQUENCY.into());
//! let mut device = SpiDeviceDriver::new(&driver, None as Option<AnyIOPin>, &config)?;
//! let mut lora = setup_lora(
//!     device,
//!     PinDriver::input_output(pins.gpio17)?.into_output()?,
//!     PinDriver::input_output(pins.gpio16)?.into_input_output()?,
//! )?;
//! // Enjoy, you have fully initialized a SX127x radio.
//!```
//!
//! Once you have a SX127x radio, you just have to define your channels (and their associated
//! [DelayParams](radio_tipe_poc::radio::DelayParams)).
//!
//!```rust,ignore
//!    const LORA_FREQUENCIES: [KiloHertz; 5] = [KiloHertz(869525),KiloHertz(867700),KiloHertz(867500),KiloHertz(867300),KiloHertz(867100)]; // EU-868MHz band
//!    
//!    // Delay params for all channels
//!    let delay_params = radio_tipe_poc::radio::DelayParams {
//!        duty_cycle: 0.01,           // 1%
//!        min_delay: 10_000_000,      // 10s
//!        poll_delay: 100_000,        // 100ms
//!        duty_interval: 120,         // 2min
//!    };
//!
//!    // The channel list
//!    let channels : Vec<radio_tipe_poc::radio::Channel<Channel>> = LORA_FREQUENCIES.into_iter().map(|freq| {
//!        let radio_channel = Channel::LoRa(LoRaChannel{
//!            freq: freq.into(),
//!            sf: SpreadingFactor::Sf9,
//!            ..Default::default()
//!        });
//!        radio_tipe_poc::radio::Channel {
//!            radio_channel,
//!            delay: delay_params.clone(),
//!        }
//!    }).collect();
//! ```
//!
//! The last thing to configure is then the ATPC, here we are going with the [TestingATPC](radio_tipe_poc::device::atpc::TestingATPC).
//! And we will have a fully functional device!
//!
//! ```rust,ignore
//!    let atpc = radio_tipe_poc::atpc::TestingATPC::new(vec![10, 8, 6, 4, 2]);
//!
//!    let mut device = LoRaRadio::new(lora, &channels, atpc, -100, None, None, 0b0101_0011);
//!```
//!
//! You can now use the [Device] implementation to actually run the protocol. Enjoy!

use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use log::info;
use radio::{Interrupts, Power, Receive, ReceiveInfo, State, Transmit};
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::{Cursor, Write};
use std::marker::PhantomData;
use std::time::{Duration, Instant, SystemTime};

use ringbuf::HeapRb;
use ringbuf::Rb;

use crate::atpc::ATPC;
use crate::device::{Device, QueueError, RxClient, TxClient};
use crate::frame::{
    self, AddressHeader, FrameNonce, FrameSize, FrameType, RadioFrameWithHeaders, RadioHeaders,
    RecipientHeader,
};
use crate::{LoRaAddress, LoRaDestination};

/// Maximum length of a frame.
///
/// Currently 5 times the [MAX_LORA_PAYLOAD] length.
const MAX_FRAME_LENGTH: usize = MAX_LORA_PAYLOAD * 5;
/// Maximum usable length of a LoRa payload (or physical frame).
///
/// Currently, 254-1. One bit is reserved for the [FrameType] discriminant.
const MAX_LORA_PAYLOAD: usize = 253;
/// Maximum number of checks to do on a channel.
///
/// If after [MAX_ATTEMPT_FREE_CHANNEL] is still not free, the [LoRaRadio] will report
/// an error [RadioError::BusyChannel].
const MAX_ATTEMPT_FREE_CHANNEL: usize = 25; // A try = 200ms wait

/// Channel representation of legal regulations on the use of electromagnetic bands.
///
/// This includes but not limits to duty cycle (max Time on Air usage on a specific period), and
/// minimum delay between transmission and poll.
#[derive(Debug, Copy, Clone)]
pub struct DelayParams {
    /// Duty cycle: usage ratio of the frequencies.
    ///
    /// The most frequent duty cycle is 1% (0.01), but please refer to your local regulations.
    pub duty_cycle: f64,
    /// Minimal delay between two transmissions (in us).
    pub min_delay: u64,
    /// Poll delay, the delay (in us) between two checks on the transmission by the physical radio.
    ///
    /// Recommended value is 100ms. Do not exceed 400ms.
    pub poll_delay: u32,
    /// Duty interval, the time period (in seconds) on which the duty cycle is defined.
    ///
    /// For instance your country might regulate transmission as 1% by hour.
    pub duty_interval: u64,
}

/// Channel super-representation, including the specific Lora Channel to use on the device and its
/// delay parameters (see [DelayParams]).
#[derive(Debug, Clone)]
pub struct Channel<C>
where
    C: Debug,
{
    /// The physical radio channel representation.
    pub radio_channel: C,
    /// The associated delay parameters to respect regulations.
    pub delay: DelayParams,
}

/// Type alias of the radio internal state.
type RadioState = radio_sx127x::device::State;

/// Radio physical device representation.
// TODO: Remove dependencies to radio_sx127x, using a generic trait with the companion types specifying
// the HAL-specific interfaces.
pub trait Radio<C, E>:
    Transmit<Error = E>
    + Receive<Info = radio_sx127x::device::PacketInfo, Error = E>
    + Power<Error = E>
    + radio::Channel<Channel = C, Error = E>
    + State<State = radio_sx127x::device::State, Error = E>
    + Interrupts<Irq = radio_sx127x::device::Interrupts, Error = E>
    + DelayMs<u32>
    + DelayUs<u32>
{
}

impl<
        C: Debug,
        E: Debug,
        T: Transmit<Error = E>
            + Receive<Info = radio_sx127x::device::PacketInfo, Error = E>
            + Power<Error = E>
            + radio::Channel<Channel = C, Error = E>
            + State<State = radio_sx127x::device::State, Error = E>
            + Interrupts<Irq = radio_sx127x::device::Interrupts, Error = E>
            + DelayMs<u32>
            + DelayUs<u32>,
    > Radio<C, E> for T
{
}

/// Device implementation for LoRa Radio module.
pub struct LoRaRadio<'a, A, T, C, E>
where
    A: ATPC,
    T: Radio<C, E>,
    C: Debug,
    E: Debug,
{
    /// The physical radio peripheral.
    radio: T,
    /// The radio channels configured for uses.
    channels: &'a [Channel<C>],
    /// Internal usage history of each channel, in order to respect regulations.
    channel_usages: Vec<(Instant, Duration)>,
    /// RSSI target, an RSSI level that allows good reception by the physical radio.
    rssi_target: i16,
    /// The ATPC to use.
    atpc: A,
    /// The (optional) reception client to which the radio acknowledges receptions.
    rx_client: Option<Box<dyn RxClient>>,
    /// The (optional) transmission client to which the radio acknowledges transmissions.
    tx_client: Option<Box<dyn TxClient>>,
    /// Internal queue of messages to transmit.
    tx_buffer: Vec<LoRaMessage>,
    /// Internal queue of acknowledgment to transmit.
    tx_buf_acknowledgments: Vec<(AddressHeader, FrameNonce, i16)>,
    /// Internal intermediate frame to transmit.
    tx_frame: Option<frame::RadioFrameWithHeaders>,
    /// Internal history of transmissions (to allow retransmissions).
    tx_history: HeapRb<frame::RadioFrameWithHeaders>,
    /// Internal queue of pending acknowledgment to transmit.
    pending_rx_acknowledgments: Vec<(AddressHeader, FrameNonce, i16)>,
    /// Internal list of awaiting acknowledgments.
    ///
    /// Each item represents a tuple of the recipient address, the nonce of the associated frame,
    /// the instant it was sent and finally a boolean indicating if this acknowledgment should
    /// update the ATPC.
    ///
    /// This last item is particularly useful when a frame is intended for more than one recipient
    /// and they do not share the same level of transmission power.
    pending_tx_acknowledgments: HeapRb<(AddressHeader, FrameNonce, Instant, bool)>,
    /// The radio address.
    ///
    /// It defines what frames the radio will listen to.
    address: LoRaAddress,
    phantom: PhantomData<E>,
}

impl<'a, A, T, C, E> LoRaRadio<'a, A, T, C, E>
where
    A: ATPC,
    T: Radio<C, E>,
    C: Debug,
    E: Debug,
{
    /// Initialize a new LoRa radio as device.
    pub fn new(
        radio: T,
        channels: &'a [Channel<C>],
        atpc: A,
        rssi_target: i16,
        rx_client: Option<Box<dyn RxClient>>,
        tx_client: Option<Box<dyn TxClient>>,
        address: LoRaAddress,
    ) -> Self {
        assert!(channels.len() > 0, "No channel declared!");
        let usages = channels
            .iter()
            .map(|_ch| (Instant::now(), Duration::ZERO))
            .collect();
        Self {
            radio,
            channels,
            atpc,
            rssi_target,
            rx_client,
            tx_client,
            address,
            tx_buffer: Vec::new(),
            tx_buf_acknowledgments: Vec::new(),
            tx_frame: None,
            channel_usages: usages,
            tx_history: HeapRb::new(60), // Tx history is limited to 60 frames, a fair limit if we consider each frame need a second to be transmit and
            // we only need this history to retransmit a packet. Acknowledgment of a packet expired after 60s.
            pending_rx_acknowledgments: Vec::new(),
            pending_tx_acknowledgments: HeapRb::new(60), // Same reason
            phantom: PhantomData,
        }
    }

    /// Builds an internal frame representation based on a buffer of messages and a buffer
    /// of acknowledgments.
    ///
    /// This function might return an error if the frame exceeds the [MAX_FRAME_LENGTH] length.
    fn build_frame(
        &self,
        buffer: &Vec<LoRaMessage>,
        tx_buf_acknowledgments: &Vec<(AddressHeader, FrameNonce, i16)>,
    ) -> Result<frame::RadioFrameWithHeaders, RadioError<E>> {
        let mut recipients: HashMap<frame::AddressHeader, frame::PayloadFlag> = HashMap::new();
        let mut payloads: Vec<frame::Payload> = Vec::new();
        // Builds the payload list and associated recipient list.
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
        // Builds the acknowledgment list and the associated recipient list.
        for (ah, _nonce, _drssi) in tx_buf_acknowledgments {
            if let None = recipients.get_mut(&(*ah).into()) {
                recipients.insert((*ah).into(), frame::PayloadFlag::new(&[]));
            }
        }
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime is before UNIX_EPOCH?!")
            .as_secs();
        let mut rp = [0u8; 2];
        // Error silenced here!
        let _ = getrandom::getrandom(&mut rp);
        let nonce = (ts << 16) + ((rp[1] as u64) << 8) + (rp[0] as u64);

        // Builds the frame based on the number of recipients.
        match recipients.len() {
            0 => Err(RadioError::InvalidRecipentsError {
                context: format!("No registered recipient!"),
            }),
            1 => {
                let headers = frame::RadioHeaders {
                    rec_n_frames: frame::InfoHeader::new(1, 0),
                    recipients: frame::RecipientHeader::Direct(recipients.iter().map(|(dest, _pf)| *dest).next().expect("First recipient does not exist while there is one recipient registered!")),
                    payloads: payloads.len() as u8,
                    sender: self.address.into(),
                    nonce,
                };
                let ffsize = headers.size() + tx_buf_acknowledgments.size();
                if ffsize > MAX_LORA_PAYLOAD {
                    return Err(RadioError::TooBigFirstFrameError { size: ffsize });
                }
                let mut frame = frame::RadioFrameWithHeaders {
                    headers,
                    acknowledgments: tx_buf_acknowledgments.clone(),
                    payloads,
                };
                let len = frame.size();
                if dbg!(len) > MAX_FRAME_LENGTH {
                    return Err(RadioError::TooBigFrameError { size: len });
                }
                let frames = dbg!((frame.size() / MAX_LORA_PAYLOAD) as u8 + 1);
                dbg!(frame.headers.rec_n_frames.set_frames(frames));
                Ok(frame)
            }
            2..=16 => {
                let headers = frame::RadioHeaders {
                    rec_n_frames: frame::InfoHeader::new(1, 0),
                    recipients: frame::RecipientHeader::Group(recipients.into_iter().collect()),
                    payloads: payloads.len() as u8,
                    sender: self.address.into(),
                    nonce,
                };
                let ffsize = headers.size() + tx_buf_acknowledgments.size();
                if ffsize > MAX_LORA_PAYLOAD {
                    return Err(RadioError::TooBigFirstFrameError { size: ffsize });
                }
                let mut frame = frame::RadioFrameWithHeaders {
                    headers,
                    acknowledgments: tx_buf_acknowledgments.clone(),
                    payloads,
                };
                let len = frame.size();
                if len > MAX_FRAME_LENGTH {
                    return Err(RadioError::TooBigFrameError { size: len });
                }
                let frames = (frame.size() / MAX_LORA_PAYLOAD) as u8 + 1;
                frame.headers.rec_n_frames.set_frames(frames);
                Ok(frame)
            }
            n => Err(RadioError::InvalidRecipentsError {
                context: format!("Too many recipients ({}, max: 16)!", n),
            }),
        }
    }
}

impl<'a, A: ATPC, C: Debug, E: Debug, T: Radio<C, E>> Device<'a> for LoRaRadio<'a, A, T, C, E> {
    type DeviceError = RadioError<E>;
    fn set_transmit_client(&mut self, client: Box<dyn TxClient>) {
        self.tx_client = Some(client);
    }
    fn set_receive_client(&mut self, client: Box<dyn RxClient>) {
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

    fn queue_acknowledgments(&mut self) -> Result<bool, QueueError<Self::DeviceError>> {
        if self.pending_rx_acknowledgments.len() > 0 {
            // Note: Hard-Limit of 16 acknowledgments by packet. This is not a hard requirement, nonetheless, in any case the acknowledgment
            // should be available in the first network frame, hence this particuliar limitation.
            let n = 16 - self.tx_buf_acknowledgments.len();
            if n > 0 {
                let take = usize::min(usize::min(16, n), self.pending_rx_acknowledgments.len());
                let mut app = self.pending_rx_acknowledgments.drain(0..take).collect();
                let mut ack_buf = self.tx_buf_acknowledgments.clone();
                ack_buf.append(&mut app);
                // Note: It might be possible to optimize a little bit more the number of acknowledgments by frame. Nonetheless, there is several parameters
                // that intervene on the size of the packet: number of recipients, the number of messages received by minute, etc.. Therefore, while it might
                // not be optimal, I'm not sure if those particular optimizations could result in significant improvements for the complexity they add.
                match self.build_frame(&self.tx_buffer, &ack_buf) {
                    Ok(frame) => {
                        self.tx_buf_acknowledgments = ack_buf;
                        self.tx_frame = Some(frame);
                        Ok(true)
                    }
                    Err(RadioError::TooBigFrameError { size }) => {
                        Err(QueueError::QueueFullError(RadioError::TooBigFrameError {
                            size,
                        }))
                    }
                    Err(err) => Err(QueueError::DeviceError(err)),
                }
            } else {
                Err(QueueError::QueueFullError(
                    RadioError::TooManyAcknowledgmentsError {
                        count: 16 + self.pending_rx_acknowledgments.len(),
                    },
                ))
            }
        } else {
            Ok(false)
        }
    }

    fn queue<'b>(
        &mut self,
        dest: LoRaDestination,
        payload: &'b [u8],
        ack: bool,
    ) -> Result<(), QueueError<Self::DeviceError>> {
        // Construct of the recipient list.
        let recipients = match dest {
            LoRaDestination::Global if ack => vec![frame::GLOBAL_ACKNOWLEDGMENT],
            LoRaDestination::Global => vec![frame::GLOBAL_NO_ACKNOWLEDGMENT],
            LoRaDestination::Unique(addr) if ack => {
                // Register the future peer (the most early possible)
                self.atpc.register_neighbor(addr);
                vec![(addr & frame::ADDRESS_BITMASK) | frame::ACKNOWLEDGMENT_BITMASK]
            }
            LoRaDestination::Unique(addr) => vec![addr & frame::ADDRESS_BITMASK],
            LoRaDestination::Group(addrs) => addrs
                .into_iter()
                .map(|addr| {
                    // Register the future peer (the most early possible)
                    self.atpc.register_neighbor(addr);
                    if ack {
                        (addr & frame::ADDRESS_BITMASK) | frame::ACKNOWLEDGMENT_BITMASK
                    } else {
                        addr & frame::ADDRESS_BITMASK
                    }
                })
                .collect(),
        };
        if recipients.len() > 48 {
            // By design, a packet can only transmit up to 48 clients at a time.
            return Err(QueueError::QueueFullError(
                RadioError::InvalidRecipentsError {
                    context: format!("Too many recipients : {}/48", recipients.len()),
                },
            ));
        }
        // Build the new packet with the future queue and check if it is valid.
        let mut buf = self.tx_buffer.clone();
        buf.push(LoRaMessage {
            dest: recipients,
            payload: payload.to_owned(),
        });
        match self.build_frame(&buf, &self.tx_buf_acknowledgments) {
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

    fn transmit(&mut self) -> Result<FrameNonce, Self::DeviceError> {
        // Ignore if no trame is available
        if let None = self.tx_frame {
            return Ok(0);
        }
        // Report busy device
        if self.is_transmitting()? {
            return Err(RadioError::BusyDevice);
        }
        let frame = self.tx_frame.as_ref().unwrap().clone(); // TODO: Clone avoidable...
        let nframes = frame.headers.rec_n_frames.get_frames() as usize;
        // Check channel availability
        println!("Transmission check");
        self.transmission_check(nframes)?;
        let bytes = frame.to_bytes();
        let mut fcursor = 0;
        let mut buf = Vec::with_capacity(MAX_LORA_PAYLOAD + 1);
        let mut last = Instant::now();
        let nonce = frame.headers.nonce;
        // ATPC: Calculate the TX power required then transmit
        println!("Transmission, selecting TX power...");
        let (tx_power, atpc_farest_peers) = {
            match &frame.headers.recipients {
                RecipientHeader::Direct(ah) => self.atpc.get_min_tx_power(vec![ah.get_address()]),
                RecipientHeader::Group(ahs) => {
                    let addrs: Vec<u16> = ahs.iter().map(|(ah, _)| ah.get_address()).collect();
                    self.atpc.get_min_tx_power(addrs)
                }
            }
        };
        self.radio
            .set_power(tx_power)
            .map_err(|src| RadioError::InternalRadioError(src))?;
        println!("Transmission starting...");
        for ch in self.channels.iter().take(nframes as usize) {
            // TODO: Better Error distinction for Internal Radio Error.
            println!("Prepare radio for the correct channel");
            self.radio
                .set_channel(&ch.radio_channel)
                .map_err(|src| RadioError::InternalRadioError(src))?;
            buf.push((FrameType::Message as u8).to_be());
            let mut end = (fcursor + 1) * MAX_LORA_PAYLOAD;
            if end > bytes.len() {
                end = bytes.len();
            }
            buf.extend_from_slice(&bytes[fcursor * MAX_LORA_PAYLOAD..end]);
            if fcursor > 0 {
                // Wait the 600ms period.
                if let Some(delay) = 600_u32.checked_sub(last.elapsed().as_millis() as u32) {
                    self.radio.delay_ms(delay);
                }
            }
            last = Instant::now();
            println!("Transmission on air");
            self.radio
                .start_transmit(&buf)
                .map_err(|src| RadioError::InternalRadioError(src))?;
            buf.clear();
            fcursor += 1;
            //self.radio.delay_ms(400); // TODO: Adapt delay to the real ToA (from Channel info),
            // currently it will be always : 400ms ToA + 200ms of space.
            while !self
                .radio
                .check_transmit()
                .map_err(|src| RadioError::InternalRadioError(src))?
            {
                println!("Transmission check");
                self.radio.delay_ms(self.channels[fcursor].delay.poll_delay);
            }
            println!("Transmission on channel successful, updating stats");
            let consumed = {
                let (clast, consumed) = self.channel_usages[fcursor];
                if clast.elapsed().as_micros() > ch.delay.duty_interval.into() {
                    Duration::from_millis(400)
                } else {
                    consumed + Duration::from_millis(400)
                }
            };
            self.channel_usages[fcursor] = (last.clone(), consumed);
            if last.elapsed().as_millis() > 600 {
                return Err(RadioError::OutOfSync{ context: format!("Frame transmission + channel change should have happened in 600ms, but it is already {}ms late.", last.elapsed().as_millis()-600)});
            }
        }
        println!("Clearing queue, acknowledging the transmission to API client");

        let _ = self.tx_history.push(frame.clone());
        match frame.headers.recipients {
            // Do not require acknowledgment for GLOBAL as we do not want a retransmission.
            RecipientHeader::Direct(ah) if ah.get_acknowledgment() && !ah.is_global() => {
                let _ = self.pending_tx_acknowledgments.push((
                    ah.clone(),
                    frame.headers.nonce.clone(),
                    last.clone(),
                    true,
                ));
            }
            RecipientHeader::Group(ahs) => {
                for (ah, _) in ahs {
                    if ah.get_acknowledgment() && !ah.is_global() {
                        // ATPC: Determine if this particular peer should be updated in the ATPC on fail reception.
                        // Reason? In group message, transmit power can be much more higher than the threshold required
                        // by another recipient, therefore we should only update the recipients with the highest threshold.
                        let should_update =
                            atpc_farest_peers.binary_search(&ah.get_address()).is_ok();
                        let _ = self.pending_tx_acknowledgments.push((
                            ah.clone(),
                            frame.headers.nonce.clone(),
                            last.clone(),
                            should_update,
                        ));
                    }
                }
            }
            _ => { /* No acknowledgment requested */ }
        }
        self.tx_frame = None;
        self.tx_buffer.clear();
        self.tx_buf_acknowledgments.clear();
        if let Some(client) = &self.tx_client {
            let _ = client.transmission_done(nonce); // TODO: Error silenced here!
        }
        Ok(nonce)
    }

    fn start_reception(&mut self) -> Result<(), Self::DeviceError> {
        // Start listening on the default channel
        self.radio
            .set_channel(&self.channels[0].radio_channel)
            .map_err(|src| RadioError::InternalRadioError(src))?;
        self.radio
            .start_receive()
            .map_err(|src| RadioError::InternalRadioError(src))?;
        Ok(())
    }

    fn check_reception(&mut self) -> Result<bool, Self::DeviceError> {
        info!("Checking missing acknowledgment...");
        let mut next = self.pending_tx_acknowledgments.pop();
        while let Some((ah, nonce, instant, update_atpc)) = next {
            if instant.elapsed() < Duration::from_secs(60) {
                let _ = self.pending_tx_acknowledgments.push_overwrite((
                    ah,
                    nonce,
                    instant,
                    update_atpc,
                ));
                break;
            }
            // ATPC: Report the missing acknowledgment as a failed reception for this peer.
            if update_atpc {
                self.atpc.report_failed_reception(ah.get_address());
            }
            if let Some(tx_client) = &self.tx_client {
                let mut frame_ = self.tx_history.pop();
                while frame_.is_some() && frame_.as_ref().unwrap().headers.nonce != nonce {
                    frame_ = self.tx_history.pop();
                }
                if let Some(frame) = frame_ {
                    let _ = self.tx_history.push(frame.clone());
                    match &frame.headers.recipients {
                        RecipientHeader::Direct(ah2) if ah.get_address() == ah2.get_address() => {
                            for pl in frame.payloads {
                                let _ = tx_client.transmission_failed(
                                    ah.get_address(),
                                    nonce.clone(),
                                    pl.clone(),
                                ); // TODO: Error silenced here!
                            }
                        }
                        RecipientHeader::Group(ahs) => {
                            for (ah2, pf) in ahs {
                                if ah.get_address() == ah2.get_address() {
                                    for mid in pf.to_message_ids() {
                                        if let Some(pl) = frame.payloads.get(mid as usize) {
                                            let _ = tx_client.transmission_failed(
                                                ah.get_address(),
                                                nonce.clone(),
                                                pl.clone(),
                                            ); // TODO: Error silenced here!
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            next = self.pending_tx_acknowledgments.pop();
        }
        info!("checking_reception...");
        if self
            .radio
            .check_receive(true)
            .map_err(|src| RadioError::InternalRadioError(src))?
        {
            info!("Received an incoming LoRa Packet.");
            let mut buf = [0u8; 256];
            if let Ok((size, packet_info)) = self.radio.get_received(&mut buf) {
                if size <= 0 {
                    info!("Packet ignored: size <= 0");
                    return Ok(false);
                }
                if buf[0] != (FrameType::Message as u8)
                    && buf[0] != (FrameType::BroadcastCheckSignal as u8)
                {
                    info!(
                        "Packet ignored: FrameType is not Message, it is {}!",
                        buf[0]
                    );
                    return Ok(false);
                }
                let (headers, _read) = RadioHeaders::try_from_bytes(&buf[1..])
                    .map_err(|src| RadioError::FrameError(src))?;
                let interest = match headers.recipients {
                    RecipientHeader::Direct(ah)
                        if ah.get_address() == self.address || ah.is_global() =>
                    {
                        true
                    }
                    RecipientHeader::Group(ahs) => {
                        if let Some((_ah, _pl)) = ahs
                            .iter()
                            .find(|(ah, _pl)| ah.get_address() == self.address || ah.is_global())
                        {
                            true
                        } else {
                            false
                        }
                    }
                    _ => {
                        info!("Message ignored because it is not addressed for us.");
                        false
                    }
                };
                if interest {
                    info!("Listening for the following packet, this frame interests us.");
                    /* TODO / WARNING (SECURITY): Note that up to this day (2022-11-13), a MITM is possible :
                    // somebody could listen for incoming frame, and short-circuit the emitting node
                    // by sending following header frame, its own crafted frames (before the emitting node do so)
                    // and take control of the payload content.
                    // If a authenticating method (or signing method) have to be added it should be added in
                    // the lead frame (otherwise the attacker can craft its own signature too) */
                    let nframes = headers.rec_n_frames.get_frames();
                    if nframes > 5 {
                        self.start_reception()?;
                        return Ok(false);
                    } // SECURITY: Do not accept arbitrary value from the outside.
                    let mut cursor = Cursor::new(Vec::with_capacity(
                        (nframes as usize * MAX_LORA_PAYLOAD) as usize,
                    ));
                    cursor.write_all(&mut buf[1..])?;
                    for ch in self.channels.iter().skip(1).take((nframes - 1) as usize) {
                        self.radio
                            .set_channel(&ch.radio_channel)
                            .map_err(|src| RadioError::InternalRadioError(src))?;
                        let mut i = 0;
                        let mut new_frame = self
                            .radio
                            .check_receive(true)
                            .map_err(|src| RadioError::InternalRadioError(src))?;
                        while !new_frame && i < 4 {
                            self.radio.delay_ms(50);
                            new_frame = self
                                .radio
                                .check_receive(true)
                                .map_err(|src| RadioError::InternalRadioError(src))?;
                            i += 1;
                        }
                        if !new_frame {
                            eprintln!("Silencing missing following frame.");
                            self.start_reception()?;
                            return Ok(false);
                        }
                        let mut buf_fp = [0u8; 256];
                        let (_size, _packet_info) = self
                            .radio
                            .get_received(&mut buf_fp)
                            .map_err(|src| RadioError::InternalRadioError(src))?;
                        cursor.write_all(&buf_fp[1..])?;
                    }
                    self.handle_message(
                        cursor.into_inner(),
                        self.rssi_target - packet_info.rssi(),
                    )?;
                    self.start_reception()?;
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    }

    // Informs the application that the ATPC/radio would like to send beacons.
    fn is_beacon_needed(&mut self) -> bool {
        return self.atpc.is_beacon_needed();
    }

    // Force the radio to send ATPC beacons.
    fn transmit_beacon(&mut self) -> Result<(), QueueError<Self::DeviceError>> {
        // Report busy device
        if self.is_transmitting()? {
            return Err(QueueError::DeviceError(RadioError::BusyDevice));
        }

        let mut tx_buf = vec![];
        tx_buf.push(LoRaMessage {
            dest: vec![frame::GLOBAL_ACKNOWLEDGMENT],
            payload: vec![],
        });

        // Check channel availability
        println!("Channel check");
        self.transmission_check(1)?;
        let mut last;

        let powers = self.atpc.get_beacon_powers();

        let mut buf = Vec::new();
        for (tpi, tp) in powers.iter().enumerate() {
            let frame = self.build_frame(&tx_buf, &Vec::new())?;
            let bytes = frame.to_bytes();
            self.radio
                .set_power(*tp)
                .map_err(|src| RadioError::InternalRadioError(src))?;
            self.atpc.register_beacon(tpi, frame.headers.nonce.clone());
            self.radio
                .set_channel(&self.channels[0].radio_channel)
                .map_err(|src| RadioError::InternalRadioError(src))?;
            buf.push((FrameType::BroadcastCheckSignal as u8).to_be());
            buf.extend_from_slice(&bytes[..]);
            last = Instant::now();
            self.radio
                .start_transmit(&buf)
                .map_err(|src| RadioError::InternalRadioError(src))?;
            buf.clear();
            while !self
                .radio
                .check_transmit()
                .map_err(|src| RadioError::InternalRadioError(src))?
            {
                println!("Transmission check");
                self.radio.delay_us(self.channels[0].delay.poll_delay);
            }
            println!("Beacon at TP {} successful, updating stats", tp);
            let consumed = {
                let (clast, consumed) = self.channel_usages[0];
                if clast.elapsed().as_secs() > self.channels[0].delay.duty_interval.into() {
                    Duration::from_millis(400)
                } else {
                    consumed + Duration::from_millis(400)
                }
            };
            if let Some(delay) = 600_u32.checked_sub(last.elapsed().as_millis() as u32) {
                self.radio.delay_ms(delay);
            }
            self.channel_usages[0] = (last.clone(), consumed);
            let _ = self.tx_history.push(frame.clone());
        }
        Ok(())
    }
}

impl<'a, A: ATPC, C: Debug, E: Debug, T: Radio<C, E>> LoRaRadio<'a, A, T, C, E> {
    /// Transmission checks, it checks that every channel can be used and that the radio channel is not busy right now.
    ///
    /// Note: it only checks that the first channel is not busy, as channels, should be use in the order by protocol
    /// assumption.
    /// Also, radio device will change its state to CAD mode and try [MAX_ATTEMPT_FREE_CHANNEL] attempts to detect an
    /// empty channel before returning an error.
    fn transmission_check(&mut self, nframes: usize) -> Result<(), RadioError<E>> {
        // Checking delay of channels
        let now = Instant::now();
        for (i, ch) in self.channels.iter().enumerate().take(nframes as usize) {
            let (last_used, consumed) = self.channel_usages[i];
            if (now - last_used) < Duration::from_secs(ch.delay.duty_interval)
                && consumed.as_secs_f64() / (ch.delay.duty_interval as f64) > ch.delay.duty_cycle
            {
                return Err(RadioError::DutyCycleConsumed);
            }
            if (now - last_used) < Duration::from_micros(ch.delay.min_delay) {
                return Err(RadioError::MinChannelDelayError);
            }
        }
        // Checking channels are available (well that the first one is available in reality based on protocol
        // assumptions).
        // For testing purposes you might want to force this value to true.
        let mut free_channel = false;
        let mut attemps = 0;
        while !free_channel && attemps < MAX_ATTEMPT_FREE_CHANNEL {
            self.radio
                .get_interrupts(true)
                .map_err(|err| RadioError::InternalRadioError(err))?;
            self.radio
                .set_state(RadioState::Cad)
                .map_err(|err| RadioError::InternalRadioError(err))?;
            match self
                .radio
                .get_interrupts(true)
                .map_err(|err| RadioError::InternalRadioError(err))?
            {
                radio_sx127x::device::Interrupts::LoRa(irqs) => {
                    free_channel = irqs.contains(radio_sx127x::device::lora::Irq::RX_TIMEOUT)
                }
                _ => {
                    return Err(RadioError::Unknown {
                        context: format!("Recieved an IRQ from an other mode than Lora!"),
                    })
                }
            }
            attemps += 1;
            self.radio.delay_ms(100);
        }
        if !free_channel {
            return Err(RadioError::BusyChannel);
        }
        return Ok(());
    }

    /// Once a message is fully receive in its entirety, this method is called to verify
    /// integrity of the message and called the needed Client and send acknowledgment.
    fn handle_message(&mut self, msg: Vec<u8>, drssi: i16) -> Result<bool, RadioError<E>> {
        // TODO: Verify integrity if implemented
        info!("Handling reception of an incoming frame.");
        let (frame, _length) = RadioFrameWithHeaders::try_from_bytes(msg.as_slice())?;
        if let Some(tx_client) = &self.tx_client {
            for (ah, nonce, drssi) in frame.acknowledgments {
                if ah.get_address() == self.address {
                    println!("DEBUG: Peer {} acknowledged the reception of message {} with a DRSSI of {} dBm", frame.headers.sender.get_address(), nonce.clone(), drssi.clone());
                    // ATPC: Report the successful reception of a frame by a peer.
                    self.atpc.report_successful_reception(
                        frame.headers.sender.get_address(),
                        nonce.clone(),
                        drssi,
                    );
                    // TxClient: Report successful reception by a peer.
                    let _ = tx_client
                        .transmission_successful(frame.headers.sender.get_address(), nonce.clone());
                    // TODO: Error silenced here.
                }
            }
        }
        if let Some(client) = &self.rx_client {
            match frame.headers.recipients {
                RecipientHeader::Direct(ah) => {
                    info!("Forwarding payloads to the RxClient.");
                    for pl in frame.payloads {
                        let _ = client.receive(
                            frame.headers.sender.get_address(),
                            pl,
                            frame.headers.nonce,
                        ); // TODO: Error silenced here!
                    }
                    if ah.get_acknowledgment() {
                        let _ = self.pending_rx_acknowledgments.push((
                            frame.headers.sender.clone(),
                            frame.headers.nonce,
                            drssi,
                        ));
                    }
                    Ok(true)
                }
                RecipientHeader::Group(ahs) => {
                    let mut reception_flag = false;
                    for (ah, pl) in ahs {
                        if !(ah.get_address() == self.address || ah.is_global()) {
                            continue;
                        }
                        info!("Forwarding payloads to the RxClient.");
                        let pls: Vec<Vec<u8>> = pl
                            .to_message_ids()
                            .iter()
                            .filter_map(|id| frame.payloads.get(*id as usize))
                            .cloned()
                            .collect();
                        println!("Debug pls: {:?}", pls);
                        if dbg!(pls.len()) < frame.headers.payloads.into() {
                            eprintln!("WARN: Badly formatted frame: missing message.");
                        }
                        for pl in pls {
                            let _ = client.receive(
                                dbg!(frame.headers.sender.get_address()),
                                dbg!(pl),
                                frame.headers.nonce,
                            ); // TODO: Error silenced here!
                        }
                        if ah.get_acknowledgment() {
                            let _ = self.pending_rx_acknowledgments.push((
                                frame.headers.sender.clone(),
                                frame.headers.nonce,
                                drssi,
                            ));
                        }
                        reception_flag = true;
                    }

                    if !reception_flag {
                        info!("Group message frame ignored because we are not a recipient.");
                        Ok(false)
                    } else {
                        Ok(true)
                    }
                }
            }
        } else {
            info!("Frame received but no RxClient connected!");
            Ok(false)
        }
    }
}

/// Internal LoRa Message representation.
#[derive(Debug, Clone)]
struct LoRaMessage {
    dest: Vec<LoRaAddress>,
    payload: Vec<u8>,
}

/// Error representation of either IO or Frame serialization errors.
#[derive(thiserror::Error, Debug)]
pub enum RadioError<R>
where
    R: Debug,
{
    /// Frame is too big to be transmitted.
    #[error("Frame is too big to be transmitted (is: {}B, max: {}B)!", .size, MAX_FRAME_LENGTH)]
    TooBigFrameError { size: usize },

    /// First frame (containing headers and acknowledgments) is too big to be transmitted.
    #[error("First frame (containing headers and acknowledgments) is too big to be transmitted (is: {}B, max: {}B)!", .size, MAX_LORA_PAYLOAD)]
    TooBigFirstFrameError { size: usize },

    /// Frame contains too much acknowledgments (more than 16) in one frame.
    #[error("Frame contains too much acknowledgments in one frame (is: {}, max: 16)!", .count)]
    TooManyAcknowledgmentsError { count: usize },

    /// Device failed to respect the sync interval.
    ///
    /// This means the radio transmitted one or more physical frames but the radio failed to respect
    /// the timing of an entire frame transmission.
    #[error("Device failed to respect the sync interval.\nContext: {}", .context)]
    OutOfSync { context: String },

    /// Invalid recipients error.
    ///
    /// It might suggest there is too many or 0 recipients or than one address is invalid.
    #[error("Invalid recipients error, might suggest there is too many or 0 recipients. One recipient might be an invalid address.\nContext: {}", .context)]
    InvalidRecipentsError { context: String },

    /// Bad frame error. See [FrameError](frame::FrameError) for more context.
    #[error("Bad frame error.")]
    FrameError(#[from] frame::FrameError),

    /// Underlying I/O Error.
    #[error("Underlying I/O Error.")]
    IoError(#[from] std::io::Error),

    /// Busy device. The radio did not answer our requests before timeout.
    #[error("Busy device.")]
    BusyDevice,

    /// Busy channel. One or more channel (that are needed for the transmission) is currently busy.
    #[error("Busy channel")]
    BusyChannel,

    /// One or more channel has consumed all of their dutycycle.
    #[error("One or more channel has consumed all of their dutycycle. Need to wait...")]
    DutyCycleConsumed,

    /// Minimum delay for one or more channel are not entirely elapsed.
    #[error("Minimum delay for one or more channel are not entirely elapsed. Need to wait...")]
    MinChannelDelayError,

    /// Internal radio error.
    #[error("Internal radio error.")]
    InternalRadioError(/*#[source]*/ R),

    /// Unknown/Unspecified radio error.
    #[error("Unknown radio error. Context: {}", .context)]
    Unknown { context: String },
}
