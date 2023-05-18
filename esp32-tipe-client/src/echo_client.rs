use anyhow::bail;
use log::warn;
use radio_tipe_poc::device::device::{Device, QueueError, RxClient, TxClient};
use radio_tipe_poc::device::frame::FrameNonce;
use radio_tipe_poc::{LoRaAddress, LoRaDestination};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A basic echo client //
pub struct EchoClient<'a, T: Device<'a>> {
    pub device: T,
    pub messages: Vec<Vec<u8>>,
    phantom: PhantomData<&'a T>,
}

impl<'a, T: Device<'a>> EchoClient<'a, T>
where
    T::DeviceError: Sync + Send + Debug + std::error::Error + 'static,
{
    pub fn new(device: T, msgs: Vec<Vec<u8>>) -> Self {
        Self {
            device,
            messages: msgs,
            phantom: PhantomData,
        }
    }

    pub fn spawn(&'a mut self) -> anyhow::Result<()> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(30);
        let mut handler = Arc::new(ProtocolHandler { sender });
        self.device.set_transmit_client(Box::new(handler.clone()));
        self.device.set_receive_client(Box::new(handler));
        {
            println!("Initializing ATPC (transmitting beacons)...");
            self.device.start_reception()?;
            self.device.transmit_beacon()?;
            self.device.start_reception()?;
            use std::sync::mpsc::RecvTimeoutError;
            let mut send_instant = Instant::now();
            let mut should_transmit = false;

            loop {
                match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(msg) => {
                        println!();
                        match msg {
                            ProtocolMessage::TransmissionDone(nonce) => {
                                println!("Successfully sent message id: {}", nonce)
                            }
                            ProtocolMessage::RecievedMessage(sender, payload, nonce) => {
                                let text = String::from_utf8_lossy(&payload);
                                println!(
                                    "Received payload (nonce:{}) from {:x}: {}",
                                    nonce, sender, text
                                );
                            }
                            ProtocolMessage::TransmissionSuccessful(rec, nonce) => println!(
                                "Recipient {} successfully received our message (nonce: {})!",
                                rec, nonce
                            ),
                            ProtocolMessage::TransmissionFailed(rec, nonce, payload) => {
                                println!("Recipient {} did not received our message (nonce: {})! Rescheduling it...", rec, nonce);
                                self.messages.push(payload);
                            }
                            _ => unimplemented!(),
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        bail!("Fatal error: radio disconnected.")
                    }
                }

                if send_instant.elapsed() > Duration::from_secs(10) || should_transmit {
                    send_instant = Instant::now();
                    if let Some(msg) = self.messages.pop() {
                        match self
                            .device
                            .queue(LoRaDestination::Unique(0b0101_0010), &msg, true)
                        {
                            Ok(_) => should_transmit = true,
                            Err(QueueError::QueueFullError(err)) => {
                                warn!("Queue full?\ncauses: {:?}", err)
                            }
                            Err(QueueError::DeviceError(err)) => return Err(err.into()),
                        };
                        let txt = String::from_utf8_lossy(&msg);
                        println!("Queue message {}", txt);
                    }
                    if should_transmit {
                        should_transmit = false;
                        let mut attempts = 0;
                        let mut transmission_nonce = None;
                        while (transmission_nonce.is_none() && attempts < 5) {
                            std::thread::sleep_ms(50);
                            attempts += 1;
                            match self.device.transmit() {
                                Ok(nonce) => transmission_nonce = Some(nonce),
                                Err(err) => println!("Transmission error:\n{:?}", err),
                            }
                        }
                        if let Some(nonce) = transmission_nonce {
                            println!("Sending message with nonce: {}", nonce);
                        }
                        self.device.start_reception()?;
                    }
                }

                if self.device.check_reception()? {
                    println!("We receive a new message :)");
                    if self.device.queue_acknowledgments()? {
                        println!("Acknowledging the received messsage.");
                        should_transmit = true;
                    }
                } else {
                    print!(".");
                }
                if self.device.is_beacon_needed() {
                    println!("Transmitting beacons (ATPC Update needed)...");
                    self.device.transmit_beacon()?;
                    self.device.start_reception()?;
                }
            }
        }
        Ok(())
    }
}

enum ProtocolMessage {
    TransmissionDone(FrameNonce),
    TransmissionSuccessful(LoRaAddress, FrameNonce),
    TransmissionFailed(LoRaAddress, FrameNonce, Vec<u8>),
    RecievedMessage(LoRaAddress, Vec<u8>, FrameNonce),
}

struct ProtocolHandler {
    sender: SyncSender<ProtocolMessage>,
}

impl TxClient for ProtocolHandler {
    fn transmission_done(&self, nonce: FrameNonce) -> Result<(), ()> {
        self.sender
            .try_send(ProtocolMessage::TransmissionDone(nonce))
            .map_err(|_| ())
    }

    fn transmission_successful(&self, recipient: LoRaAddress, nonce: FrameNonce) -> Result<(), ()> {
        self.sender
            .try_send(ProtocolMessage::TransmissionSuccessful(recipient, nonce))
            .map_err(|_| ())
    }

    fn transmission_failed(
        &self,
        recipient: LoRaAddress,
        nonce: FrameNonce,
        payload: Vec<u8>,
    ) -> Result<(), ()> {
        self.sender
            .try_send(ProtocolMessage::TransmissionFailed(
                recipient, nonce, payload,
            ))
            .map_err(|_| ())
    }
}

impl RxClient for ProtocolHandler {
    fn receive(&self, sender: LoRaAddress, payload: Vec<u8>, nonce: FrameNonce) -> Result<(), ()> {
        self.sender
            .try_send(ProtocolMessage::RecievedMessage(sender, payload, nonce))
            .map_err(|_| ())
    }
}
