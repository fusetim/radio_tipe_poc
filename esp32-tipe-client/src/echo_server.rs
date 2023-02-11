use radio_tipe_poc::device::device::{Device, QueueError, TxClient, RxClient};
use radio_tipe_poc::device::frame::{FrameNonce};
use radio_tipe_poc::LoRaAddress;
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use anyhow::bail;
use radio_tipe_poc::LoRaDestination;
use std::time::{Duration, Instant};
use std::marker::PhantomData;
use log::warn;
use std::fmt::Debug;
use std::sync::Arc;

/// A basic echo server 
pub struct EchoServer<'a,T: Device<'a>> {
    pub device: T,
    phantom: PhantomData<&'a T>,
}

impl<'a, T: Device<'a>> EchoServer<'a, T> 
    where T::DeviceError: Sync + Send + Debug + std::error::Error + 'static
{
    pub fn new(device: T) -> Self {
        Self {
            device,
            phantom: PhantomData,
        }
    }

    pub fn spawn(&'a mut self) -> anyhow::Result<()> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(30);
        let mut handler = Arc::new(ProtocolHandler {
            sender,       
        });
        self.device.set_receive_client(Box::new(handler));
        {
            self.device.start_reception()?;
            use std::sync::mpsc::RecvTimeoutError;
            let mut c = 0;
            loop {
                match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(msg) => {
                        println!();
                        match msg {
                            ProtocolMessage::TransmissionDone(nonce) => println!("Successfully sent message id: {}", nonce),
                            ProtocolMessage::RecievedMessage(sender, payload, nonce) => {
                                let text = String::from_utf8_lossy(&payload);
                                println!("Received payload (nonce:{}) from {:x}: {}", nonce, sender, text);
                                let dest = LoRaDestination::Unique(sender);
                                match self.device.queue(dest, &payload, false) {
                                    Ok(_) => {},
                                    Err(QueueError::QueueFullError(err)) => eprintln!("WARN: Queue full?\ncauses: {:?}", err),
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
                                    println!("Echo message sended (nonce: {})...", nonce);
                                }
                                self.device.start_reception()?;
                            },
                            ProtocolMessage::AcknowledgmentMessage(..) => unimplemented!(),
                        }
                    },
                    Err(RecvTimeoutError::Timeout) => {},
                    Err(RecvTimeoutError::Disconnected) => bail!("Fatal error: radio disconnected."), // TODO: Might do better error
                }
                if self.device.check_reception()? {
                    println!("We receive a new message :)");
                    c=0;
                } else {
                    c+=1;
                    print!(".");
                    if c >= 20 {
                        c = 0;
                        println!();
                    }
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
    fn send_done(&self, nonce: FrameNonce) -> Result<(), ()> {
        self.sender.try_send(ProtocolMessage::TransmissionDone(nonce)).map_err(|_| ())
    }
}

impl RxClient for ProtocolHandler {
    fn receive(&self, sender: LoRaAddress, payload: Vec<u8>, nonce: FrameNonce) -> Result<(), ()> {
        println!("RxClient received message {} from {}!", nonce, sender);
        self.sender.try_send(ProtocolMessage::RecievedMessage(sender, dbg!(payload), nonce)).map_err(|_| ())
    }
}
