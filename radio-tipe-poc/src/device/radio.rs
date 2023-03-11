use embedded_hal::blocking::delay::{DelayMs, DelayUs};
use radio::{Interrupts, Power, Receive, State, Transmit};
use std::collections::HashMap;
use std::fmt::Debug;
use log::{info, warn, debug, trace, log_enabled};
use log::Level;

use ringbuf::Rb;
use ringbuf::HeapRb;

use super::device::{Device, QueueError, RxClient, TxClient};
use crate::{LoRaAddress, LoRaDestination};
use std::marker::PhantomData;
use std::io::Write;
use crate::device::frame::FrameNonce;
use crate::device::frame::AddressHeader;

use std::time::{Duration, Instant};
use std::io::Cursor;

use super::frame;
use frame::{FrameSize, FrameType, RadioHeaders, RadioFrameWithHeaders, RecipientHeader};

const MAX_FRAME_LENGTH: usize = MAX_LORA_PAYLOAD * 5;
const MAX_LORA_PAYLOAD: usize = 253;
const MAX_ATTEMPT_FREE_CHANNEL: usize = 25; // A try = 200ms wait

/// Channel representation of legal regulations on the use of electromagnetic bands.
///
/// This includes but not limits to duty cycle (max Time on Air usage on a specific period), and
/// minimum delay between transmission and poll.
#[derive(Debug, Copy, Clone)]
pub struct DelayParams {
    pub duty_cycle: f64,
    pub min_delay: u64,     //us
    pub poll_delay: u64,    //us
    pub duty_interval: u64, //us
}

/// Channel super-representation, including the specific Lora Channel to use on the device and its
/// delay parameters (see [DelayParams]).
#[derive(Debug, Clone)]
pub struct Channel<C>
where
    C: Debug,
{
    pub radio_channel: C,
    pub delay: DelayParams,
}

type RadioState = radio_sx127x::device::State;

/// Radio physical device representation.
//
// TODO: Remove dependencies to radio_sx127x, using a generic trait with the companion types specifying
// the HAL-specific interfaces.
/*
pub trait RadioHal {
    type PacketInfo;
    type State;
    type Irq;
}
*/
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
pub struct LoRaRadio<'a, T, C, E>
where
    T: Radio<C, E>,
    C: Debug,
    E: Debug,
{
    radio: T,
    channels: &'a [Channel<C>],
    channel_usages: Vec<(Instant, Duration)>,
    rx_client: Option<Box<dyn RxClient>>,
    tx_client: Option<Box<dyn TxClient>>,
    tx_buffer: Vec<LoRaMessage>,
    tx_buf_acknowledgements: Vec<(AddressHeader, FrameNonce)>,
    tx_frame: Option<frame::RadioFrameWithHeaders>,
    tx_history: HeapRb<frame::RadioFrameWithHeaders>,
    pending_rx_acknowledgements: Vec<(AddressHeader, FrameNonce)>,
    pending_tx_acknowledgements: HeapRb<(AddressHeader, FrameNonce, Instant)>,
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
        channels: &'a [Channel<C>],
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
            rx_client,
            tx_client,
            address,
            tx_buffer: Vec::new(),
            tx_buf_acknowledgements: Vec::new(),
            tx_frame: None,
            channel_usages: usages,
            tx_history: HeapRb::new(60), // Tx history is limited to 60 frames, a fair limit if we consider each frame need a second to be transmit and 
                                         // we only need this history to retransmit a packet. Acknowledgement of a packet expired after 60s.
            pending_rx_acknowledgements: Vec::new(),
            pending_tx_acknowledgements: HeapRb::new(60), // Same reason
            phantom: PhantomData,
        }
    }

    fn build_trame(
        &self,
        buffer: &Vec<LoRaMessage>,
        tx_buf_acknowledgements: &Vec<(AddressHeader, FrameNonce)>,
    ) -> Result<frame::RadioFrameWithHeaders, RadioError<E>> {
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
        for (ah, nonce) in tx_buf_acknowledgements {
            if let None = recipients.get_mut(&(*ah).into()) {
                recipients.insert((*ah).into(), frame::PayloadFlag::new(&[]));
            }
        }
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
                    nonce: 0x1001, // TODO: implement nonce!!
                };
                let ffsize = headers.size() + tx_buf_acknowledgements.size();
                if ffsize > MAX_LORA_PAYLOAD {
                    return Err(RadioError::TooBigFirstFrameError { size: ffsize });
                }
                let mut frame = frame::RadioFrameWithHeaders { headers, acknowledgements: tx_buf_acknowledgements.clone(), payloads };
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
                    nonce: 0, // TODO: Implement nonce!!
                };
                let ffsize = headers.size() + tx_buf_acknowledgements.size();
                if ffsize > MAX_LORA_PAYLOAD {
                    return Err(RadioError::TooBigFirstFrameError { size: ffsize });
                }
                let mut frame = frame::RadioFrameWithHeaders { headers, acknowledgements: tx_buf_acknowledgements.clone(), payloads };
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

impl<'a, C: Debug, E: Debug, T: Radio<C, E>> Device<'a> for LoRaRadio<'a, T, C, E> {
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

    fn queue_acknowledgements(
        &mut self,
    ) -> Result<bool, QueueError<Self::DeviceError>> {
        if self.pending_rx_acknowledgements.len() > 0 {
            // Note: Hard-Limit of 16 acknowledgements by packet. This is not a hard requirement, nonetheless, in any case the acknowledgement 
            // should be available in the first network frame, hence this particuliar limitation.
            let n = 16 - self.tx_buf_acknowledgements.len();
            if n > 0 {
                let mut app = self.pending_rx_acknowledgements.drain(0..n).collect();
                let mut ack_buf = self.tx_buf_acknowledgements.clone();
                ack_buf.append(&mut app);
                // Note: It might be possible to optimize a little bit more the number of acknowledgements by frame. Nonetheless, there is several parameters
                // that intervene on the size of the packet: number of recipients, the number of messages received by minute, etc.. Therefore, while it might
                // not be optimal, I'm not sure if those particular optimizations could result in significant improvements for the complexity they add.
                match self.build_trame(&self.tx_buffer, &ack_buf) {
                    Ok(frame) => {
                        self.tx_buf_acknowledgements = ack_buf;
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
                Err(QueueError::QueueFullError(RadioError::TooManyAcknowledgementsError{count: 16 + self.pending_rx_acknowledgements.len() }))
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
        match self.build_trame(&buf, &self.tx_buf_acknowledgements) {
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
        println!("Transmission starting...");
        for ch in self.channels.iter().take(nframes as usize) {
            // TODO: Better Error distinctionbuf[0 for Internal Radio Error.
            println!("Prepare radio for the correct channel");
            self.radio
                .set_channel(&ch.radio_channel)
                .map_err(|src| RadioError::InternalRadioError(src))?;
            buf.push((FrameType::Message as u8).to_be());
            let mut end = (fcursor + 1) * MAX_LORA_PAYLOAD;
            if end > bytes.len() {
                end = bytes.len();
            }
            buf.extend_from_slice(
                &bytes[fcursor * MAX_LORA_PAYLOAD..end],
            );
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
                self.radio.delay_ms(100);
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

        self.tx_history.push(frame.clone());
        match frame.headers.recipients {
            RecipientHeader::Direct(ah) if ah.get_acknowledgment() => {
                self.pending_tx_acknowledgements.push((ah.clone(), frame.headers.nonce.clone(), last.clone()));
            },
            RecipientHeader::Group(ahs) => {
                for (ah,_) in ahs {
                    if ah.get_acknowledgment() {
                        self.pending_tx_acknowledgements.push((ah.clone(), frame.headers.nonce.clone(), last.clone()));
                    } 
                }
            },
            _ => { /* No acknowledgement requested */ },
        }
        self.tx_frame = None;
        self.tx_buffer.clear();
        self.tx_buf_acknowledgements.clear();
        if let Some(client) = &self.tx_client {
            client.transmission_done(nonce); // TODO: Error silenced here!
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
        info!("Checking missing acknowledgement...");
        let mut next = self.pending_tx_acknowledgements.pop();
        while let Some((ah, nonce, instant)) = next {
            if instant.elapsed() < Duration::from_secs(60) {
                let _ = self.pending_tx_acknowledgements.push_overwrite((ah, nonce, instant));
                break;
            }
            if let Some(tx_client) = &self.tx_client {
                let mut frame_ = self.tx_history.pop();
                while frame_.is_some() && frame_.as_ref().unwrap().headers.nonce != nonce {
                    frame_ = self.tx_history.pop();
                }
                if let Some(frame) = frame_ {
                    self.tx_history.push(frame.clone());
                    match &frame.headers.recipients { // TODO/SECURITY: Should we acknowledge Global message? Sounds like it enable DDOS attacks.
                        RecipientHeader::Direct(ah2) if ah.get_address() == ah2.get_address() => {
                            for pl in frame.payloads {
                                let _ = tx_client.transmission_failed(ah.get_address(), nonce.clone(), pl.clone()); // TODO: Error silenced here!
                            }
                        },
                        RecipientHeader::Group(ahs) => {
                            for (ah2, pf) in ahs {
                                if ah.get_address() == ah2.get_address() {
                                    for mid in pf.to_message_ids() {
                                        if let Some(pl) = frame.payloads.get(mid as usize) {
                                            let _ = tx_client.transmission_failed(ah.get_address(), nonce.clone(), pl.clone()); // TODO: Error silenced here!
                                        }
                                    }
                                }
                            }
                        }
                        _ => {},
                    }
                }
            }    
            next = self.pending_tx_acknowledgements.pop();
        }
        info!("checking_reception...");
        if self
            .radio
            .check_receive(true)
            .map_err(|src| RadioError::InternalRadioError(src))?
        {
            info!("Received an incoming LoRa Packet.");
            let mut buf = [0u8; 256];
            if let Ok((size, _packet_info)) = self.radio.get_received(&mut buf) {
                // TODO: use the packet_info metadata like RSSI to calculate ATRP.
                if size <= 0 { 
                    info!("Packet ignored: size <= 0");
                    return Ok(false);
                }
                if buf[0] != (FrameType::Message as u8) && buf[0] != (FrameType::RelayMessage as u8)
                {
                    // TODO: Handle other frame types
                    // For now, it is ignored as not a inbound message.
                    info!("Packet ignored: FrameType is not (Relay-)Message, it is {}!", buf[0]);
                    return Ok(false);
                }
                let (headers, _read) = RadioHeaders::try_from_bytes(&buf[1..])
                    .map_err(|src| RadioError::FrameError(src))?;
                let interest = match headers.recipients {
                    RecipientHeader::Direct(ah) if ah.get_address() == self.address || ah.is_global() => {
                        true
                    }
                    RecipientHeader::Group(ahs) => {
                        if let Some((_ah, _pl)) =
                            ahs.iter().find(|(ah, _pl)| ah.get_address() == self.address || ah.is_global())
                        {
                            true    
                        } else { false }
                    }
                    _ => {
                        // TODO: Implement relay logic there.
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
                    let mut cursor = Cursor::new(Vec::with_capacity((nframes as usize * MAX_LORA_PAYLOAD) as usize));
                    cursor.write_all(&mut buf[1..])?;
                    for ch in self.channels.iter().skip(1).take((nframes-1) as usize){
                        self.radio
                            .set_channel(&ch.radio_channel)
                            .map_err(|src| RadioError::InternalRadioError(src))?;
                        let mut i = 0;
                        let mut new_frame = self.radio
                                .check_receive(true)
                                .map_err(|src| RadioError::InternalRadioError(src))?;
                        while !new_frame && i < 4 {
                            self.radio.delay_ms(50);
                            new_frame = self.radio
                                .check_receive(true)
                                .map_err(|src| RadioError::InternalRadioError(src))?;
                            i+=1;
                        }
                        if !new_frame {
                            eprintln!("Silencing missing following frame.");
                            self.start_reception()?;
                            return Ok(false);
                        }
                        let mut buf_fp = [0u8; 256];
                        // TODO: use the packet_info metadata like RSSI to calculate ATRP.
                        let (_size, _packet_info) = self.radio.get_received(&mut buf_fp).map_err(|src| RadioError::InternalRadioError(src))?;
                        cursor.write_all(&buf_fp[1..])?;
                    }
                    self.handle_message(cursor.into_inner())?;
                    self.start_reception()?;
                    return Ok(true)
                }
            }
        }
        return Ok(false);
    }
}

impl<'a, C: Debug, E: Debug, T: Radio<C, E>> LoRaRadio<'a, T, C, E> {
    /// Transmission checks, it checks that every channel can be used and that the radio channel is not busy right now.
    ///
    /// Note: it only checks that the first channel is not busy, as channels, should be use in the order by protocol
    /// assumption.
    /// Also, radio device will change its state to CAD mode and try MAX_ATTEMPT_FREE_CHANNEL attempts to detect an
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
        let mut free_channel = true; // TODO: For testing purposes only!!
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
    fn handle_message(&mut self, msg: Vec<u8>) -> Result<bool, RadioError<E>> {
        // TODO: Verify integrity if implemented
        info!("Handling reception of an incoming frame.");
        let (frame, _length) = RadioFrameWithHeaders::try_from_bytes(msg.as_slice())?;
        if let Some(tx_client) = &self.tx_client {
            for (ah, nonce) in frame.acknowledgements {
                if ah.get_address() == self.address {
                    let _ = tx_client.transmission_successful(frame.headers.sender.get_address(), nonce.clone()); // TODO: Error silenced here.
                }
            }
        }
        if let Some(client) = &self.rx_client {
            match frame.headers.recipients {
                RecipientHeader::Direct(ah) => {
                    info!("Forwarding payloads to the RxClient.");
                    for pl in frame.payloads {
                        let _ = client.receive(frame.headers.sender.get_address(), pl, frame.headers.nonce); // TODO: Error silenced here!
                    }
                    if ah.get_acknowledgment() {
                        let _ = self.pending_rx_acknowledgements.push((frame.headers.sender.clone(), frame.headers.nonce));
                    }
                    Ok(true)
                }
                RecipientHeader::Group(ahs) => {
                    if let Some((ah, pl)) =
                        ahs.iter().find(|(ah, _pl)| ah.get_address() == self.address || ah.is_global()) // TODO/BUG: Use filter not find!!
                    {
                        info!("Forwarding payloads to the RxClient.");
                        let pls : Vec<Vec<u8>> = pl.to_message_ids().iter().filter_map(|id| frame.payloads.get(*id as usize)).cloned().collect();
                        println!("Debug pls: {:?}", pls);
                        if dbg!(pls.len()) < frame.headers.payloads.into() {
                            eprintln!("WARN: Badly formatted frame: missing message.");
                        }
                        for pl in pls {
                            let _ = client.receive(dbg!(frame.headers.sender.get_address()), dbg!(pl), frame.headers.nonce); // TODO: Error silenced here!
                        } 
                        if ah.get_acknowledgment() {
                            let _ = self.pending_rx_acknowledgements.push((frame.headers.sender.clone(), frame.headers.nonce));
                        }
                        Ok(true)
                    } else {
                        info!("Group message frame ignored because we are not a recipient.");
                        Ok(false)
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

#[derive(thiserror::Error, Debug)]
pub enum RadioError<R>
where
    R: Debug,
{
    #[error("Frame is too big to be transmitted (is: {}B, max: {}B)!", .size, MAX_FRAME_LENGTH)]
    TooBigFrameError { size: usize },

    #[error("First frame (containing headers and acknowledgements) is too big to be transmitted (is: {}B, max: {}B)!", .size, MAX_LORA_PAYLOAD)]
    TooBigFirstFrameError { size: usize },

    #[error("Frame contains too much acknowledgements in one frame (is: {}, max: 16)!", .count)]
    TooManyAcknowledgementsError { count: usize },

    #[error("Device failed to respect the sync interval.\nContext: {}", .context)]
    OutOfSync { context: String },

    #[error("Invalid recipients error, might suggest there is too many or 0 recipients. One recipient might be an invalid address.\nContext: {}", .context)]
    InvalidRecipentsError { context: String },

    #[error("Bad frame error.")]
    FrameError(#[from] frame::FrameError),

    #[error("Underlying I/O Error.")]
    IoError(#[from] std::io::Error),

    #[error("Busy device.")]
    BusyDevice,

    #[error("Busy channel")]
    BusyChannel,

    #[error("One or more channel has consumed all of their dutycycle. Need to wait...")]
    DutyCycleConsumed,

    #[error("Minimum delay for one or more channel are not entirely elapsed. Need to wait...")]
    MinChannelDelayError,

    #[error("Internal radio error.")]
    InternalRadioError(/*#[source]*/ R),

    #[error("Unknown radio error. Context: {}", .context)]
    Unknown { context: String },
}
