use anyhow::bail;
use log::warn;
use radio_tipe_poc::device::{Device, QueueError, RxClient, TxClient};
use radio_tipe_poc::frame::FrameNonce;
use radio_tipe_poc::LoRaAddress;
use radio_tipe_poc::LoRaDestination;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A basic echo server
pub struct EchoServer<'a, T: Device<'a>> {
    pub device: T,
    phantom: PhantomData<&'a T>,
}

impl<'a, T: Device<'a>> EchoServer<'a, T>
where
    T::DeviceError: Sync + Send + Debug + std::error::Error + 'static,
{
    pub fn new(device: T) -> Self {
        Self {
            device,
            phantom: PhantomData,
        }
    }

    fn try_transmit(&mut self) -> anyhow::Result<()> {
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
            println!("Echo message sended (nonce: {})...", nonce);
        }
        Ok(())
    }

    pub fn spawn(&'a mut self) -> anyhow::Result<()> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(30);
        let mut handler = Arc::new(ProtocolHandler { sender });
        self.device.set_transmit_client(Box::new(handler.clone()));
        self.device.set_receive_client(Box::new(handler));
        {
            println!("Initializing ATPC (transmitting beacons)...");
            self.device.start_reception()?;
            //self.device.transmit_beacon()?;
            self.device.start_reception()?;

            use std::sync::mpsc::RecvTimeoutError;
            let mut c = 0;
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
                                    "Received payload (nonce:{}) from {}: {}",
                                    nonce, sender, text
                                );
                                let dest = LoRaDestination::Unique(sender);
                                match self.device.queue(dest, &payload, false) {
                                    Ok(_) => {}
                                    Err(QueueError::QueueFullError(err)) => {
                                        eprintln!("WARN: Queue full?\ncauses: {:?}", err);
                                        self.try_transmit()?;
                                        should_transmit = false;
                                        self.device.start_reception()?;
                                    }
                                    Err(QueueError::DeviceError(err)) => return Err(err.into()),
                                };
                                should_transmit = true;
                            }
                            ProtocolMessage::TransmissionSuccessful(rec, nonce) => println!(
                                "Recipient {} successfully received our message (nonce: {})!",
                                rec, nonce
                            ),
                            ProtocolMessage::TransmissionFailed(rec, nonce, payload) => {
                                println!("Recipient {} did not received our message (nonce: {})! Rescheduling it...", rec, nonce);
                                let dest = LoRaDestination::Unique(rec);
                                match self.device.queue(dest, &payload, false) {
                                    Ok(_) => {}
                                    Err(QueueError::QueueFullError(err)) => {
                                        eprintln!("WARN: Queue full?\ncauses: {:?}", err);
                                        self.try_transmit()?;
                                        should_transmit = false;
                                        self.device.start_reception()?;
                                    }
                                    Err(QueueError::DeviceError(err)) => return Err(err.into()),
                                };
                                should_transmit = true;
                            }
                            _ => unimplemented!(),
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        bail!("Fatal error: radio disconnected.")
                    }
                }
                if self.device.check_reception()? {
                    println!("We receive a new message :)");
                    if self.device.queue_acknowledgments()? {
                        println!("Acknowledging the received messsage.");
                        should_transmit = true;
                    }
                    c = 0;
                } else {
                    c += 1;
                    print!(".");
                    if c >= 20 {
                        c = 0;
                        if should_transmit {
                            self.try_transmit()?;
                            should_transmit = false;
                            self.device.start_reception()?;
                        }
                        println!();
                    }
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
        println!("RxClient received message {} from {}!", nonce, sender);
        self.sender
            .try_send(ProtocolMessage::RecievedMessage(
                sender,
                dbg!(payload),
                nonce,
            ))
            .map_err(|_| ())
    }
}
