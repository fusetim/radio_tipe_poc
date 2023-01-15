use radio_tipe_poc::device::device::{Device, QueueError, TxClient, RxClient};
use radio_tipe_poc::device::frame::{FrameNonce};
use radio_tipe_poc::{LoRaAddress, LoRaDestination};
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use anyhow::bail;
use log::warn;

use std::marker::PhantomData;
use std::time::{Instant, Duration};
use std::fmt::Debug;


/// A basic echo client //
pub struct EchoClient<'a, T: Device<'a>> {
    pub device: T,
    pub messages: Vec<Vec<u8>>,
    phantom: PhantomData<&'a T>,
}

impl<'a, T: Device<'a>> EchoClient<'a, T> 
    where T::DeviceError: Sync + Send + Debug + std::error::Error + 'static
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
        let handler = ProtocolHandler {
            sender,       
        };
        {
            self.device.start_reception()?;
            use std::sync::mpsc::RecvTimeoutError;
 
            let mut send_instant = Instant::now();

            loop {
                match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(msg) => {
                        println!();
                        match msg {
                            ProtocolMessage::TransmissionDone(nonce) => println!("Successfully sent message id: {}", nonce),
                            ProtocolMessage::RecievedMessage(sender, payload, nonce) => {
                                let text = String::from_utf8_lossy(&payload);
                                println!("Received payload (nonce:{}) from {:x}: {}", nonce, sender, text);
                            }
                            ProtocolMessage::AcknowledgmentMessage(..) => unimplemented!(),
                        }
                    },
                    Err(RecvTimeoutError::Timeout) => {},
                    Err(RecvTimeoutError::Disconnected) => bail!("Fatal error: radio disconnected."), // TODO: Might do better error
                }

                if send_instant.elapsed() > Duration::from_secs(10) && !self.messages.is_empty(){
                    send_instant = Instant::now();
                    if let Some(msg) = self.messages.pop() {
                        match self.device.queue(LoRaDestination::Unique(0b0101_0010), &msg, false) {
                            Ok(_) => {},
                            Err(QueueError::QueueFullError(err)) => warn!("Queue full?\ncauses: {:?}", err),
                            Err(QueueError::DeviceError(err)) => return Err(err.into()),
                        };
                        let mut attempts = 0;
                        let mut transmission_nonce = None;
                        while (transmission_nonce.is_none() && attempts < 50) {
                            attempts+=1;
                            match self.device.transmit() {
                                Ok(nonce) => transmission_nonce = Some(nonce),
                                Err(err) => println!("Transmission error:\n{:?}", err),
                            }
                        }
                        if let Some(nonce) = transmission_nonce {
                            let txt = String::from_utf8_lossy(&msg);
                            println!("Sending message (nonce: {}): {}", nonce, txt);
                        }                        
                        self.device.start_reception()?;
                    }
                }

                if self.device.check_reception()? {
                    println!("We receive a new message :)");
                } else {
                    print!(".");
                }
            }
        }
        Ok(())
    }
}

enum ProtocolMessage {
    TransmissionDone(FrameNonce),
    RecievedMessage(LoRaAddress, Vec<u8>, FrameNonce),
    AcknowledgmentMessage(FrameNonce, LoRaAddress),
}

struct ProtocolHandler {
    sender: SyncSender<ProtocolMessage>,
}

impl TxClient for ProtocolHandler {
    fn send_done(&mut self, nonce: FrameNonce) -> Result<(), ()> {
        self.sender.try_send(ProtocolMessage::TransmissionDone(nonce)).map_err(|_| ())
    }
}

impl RxClient for ProtocolHandler {
    fn receive(&mut self, sender: LoRaAddress, payload: Vec<u8>, nonce: FrameNonce) -> Result<(), ()> {
        self.sender.try_send(ProtocolMessage::RecievedMessage(sender, payload, nonce)).map_err(|_| ())
    }
}