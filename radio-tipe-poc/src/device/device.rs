//! Definitions for the abstract device driver.
//!
//! It is the essential trait that all applications will have to use to interact with
//! the radio.
//!
//! ## Usages
//!
//! Here is a very short example of how to use [Device] to exchange messages.
//!
//! In most cases, it will run in a infinite loop to poll and push messages to the network.
//!
//! ```rust,ignore
//! pub fn spawn(&'a mut self) -> anyhow::Result<()> {
//!     // Create a Tx/Rx Client if necessary
//!     let handler = ...;
//!
//!     self.device.set_transmit_client(Box::new(handler.clone()));
//!     self.device.set_receive_client(Box::new(handler));
//!    
//!     {
//!         use std::sync::mpsc::RecvTimeoutError;
//!         let mut should_transmit = false;
//!
//!         println!("Initializing ATPC (transmitting beacons)...");
//!         self.device.start_reception()?;
//!         self.device.transmit_beacon()?;
//!         self.device.start_reception()?;
//!
//!         loop {
//!             // Do something that might set should_transmit to true.
//!             // Maybe consume message from the Tx/Rx Client?
//!
//!             // Checks for reception, processes acknowledgment.
//!             if self.device.check_reception()? {
//!                 println!("We receive a new message :)");
//!                 if self.device.queue_acknowledgements()? {
//!                     println!("Acknowledging the received messsage.");
//!                     should_transmit = true;
//!                 }
//!             } else {
//!                 // When there is a hint that a transmission should happen, try to transmit.
//!                 if should_transmit {
//!                     self.try_transmit()?;
//!                     should_transmit = false;
//!                     self.device.start_reception()?;
//!                 }
//!             }
//!
//!             // When needed, send the initialization beacons.
//!             if self.device.is_beacon_needed() {
//!                 println!("Transmitting beacons (ATPC Update needed)...");
//!                 self.device.transmit_beacon()?;
//!                 self.device.start_reception()?;
//!             }
//!         }
//!     }
//!     Ok(())
//! }
//! ```

use crate::device::frame::FrameNonce;
use crate::{LoRaAddress, LoRaDestination};
use std::sync::Arc;

/// Wrapper for an error that might be indicated a full queue.
#[derive(thiserror::Error, Debug)]
pub enum QueueError<T> {
    /// This error is due to other reasons than a full queue.
    #[error("Internal device error. Error not linked to queue being full, no need to transmit.")]
    DeviceError(#[from] T),
    /// This error results from a full queue. The queue must be cleared (by transmitting for instance)
    /// before you call again the function.
    #[error("Queue is full. Transmit first to clear the queue and try again.")]
    QueueFullError(#[source] T),
}

/// Device trait represents a unit system that can receive and send messages using
/// some complex features like Adaptive-Rate-Power-Rate, Acknowledgment or Packet Aggregation.
///
/// TODO: Give default implementation for most of the inner method when they are not related to
/// a specifi radio implementation.
///
/// TODO: Implement a Mock device using the MockRadio provided by the radio crate.
///
/// A small example is available at the [module level](crate::device::device).
pub trait Device<'a> {
    type DeviceError;

    /// Register the new transmission client which will recieve packet acknowledgment and
    /// transmission completion signal.
    fn set_transmit_client(&mut self, client: Box<dyn TxClient>);

    /// Register the new reciever client which will be call for every packet received matching
    // the device address.
    fn set_receive_client(&mut self, client: Box<dyn RxClient>);

    /// Register this device with a new address.
    fn set_address(&mut self, address: LoRaAddress);

    /// Retrieve the current registered address for this device.
    fn get_address(&self) -> LoRaAddress;

    /// Get transmission status.
    fn is_transmitting(&mut self) -> Result<bool, Self::DeviceError>;

    /// Get listening status.
    fn is_listening(&mut self) -> Result<bool, Self::DeviceError>;

    /// Flush the packet queue and transmit it using its current state.
    ///
    /// NO-OP if the queue is empty.
    fn transmit(&mut self) -> Result<FrameNonce, Self::DeviceError>;

    /// Put the device in listening mode, waiting to recieve new packets on its address.
    ///
    /// Periodical check need to be made with [Device::check_reception] to poll internal radio state
    /// and retrieve the received message by the physical device.
    fn start_reception(&mut self) -> Result<(), Self::DeviceError>;

    /// Check reception of messages by the physical radio.
    ///
    /// Periodical check need to be made with this method to poll internal radio state
    /// and retrieve the received message by the physical device.
    ///
    /// Note that this method can fail if the physical radio is not in reception mode (you should use
    /// [[Device::start_reception]] for that). Ypu might check this mode by using [Device::is_listening].
    ///
    /// Please note that you **MUST** acknowledge successful reception by calling [Device::queue_acknowledgements] in the
    /// 60s following this call. This is not done automatically by design, to allow packet aggregation and avoid
    /// a transmission in a function called "reception".
    fn check_reception(&mut self) -> Result<bool, Self::DeviceError>;

    /// Queue and prepare acknowledgements (due to a successful reception) for the next frame.
    ///
    /// Returns [QueueError], on [QueueError::QueueFullError] queue need to be flush and transmit
    /// before being able to call again this function.
    fn queue_acknowledgements(&mut self) -> Result<bool, QueueError<Self::DeviceError>>;

    /// Add given payload as packet to the internal queue.
    ///
    /// Returns [QueueError], on [QueueError::QueueFullError] queue need to be flush and transmit
    /// before appending new packets.
    fn queue<'b>(
        &mut self,
        dest: LoRaDestination,
        payload: &'b [u8],
        ack: bool,
    ) -> Result<(), QueueError<Self::DeviceError>>;

    /// Informs the application that the ATPC/radio would like to send beacons.
    fn is_beacon_needed(&mut self) -> bool;

    /// Forces the radio to send ATPC beacons.
    fn transmit_beacon(&mut self) -> Result<(), QueueError<Self::DeviceError>>;
}

/// Transmission client, that acts like a callback on transmission of a message.
///
/// Device will call this function to acknowledge completion and/or reception
/// of a previously queued payload.
pub trait TxClient {
    /// Device acknowledgment of transmission completed
    fn transmission_done(&self, nonce: FrameNonce) -> Result<(), ()>;

    /// Transmission was successful, got an acknowledgement from the given recipient for this particular message.
    fn transmission_successful(&self, recipient: LoRaAddress, nonce: FrameNonce) -> Result<(), ()>;

    /// Transmission failed, while an acknowledgement was required, none was received by the device from the given recipient for this
    /// particular message.
    /// A retransmission can be asked by using [[Device::queue]] with the passed payload.
    fn transmission_failed(
        &self,
        sender: LoRaAddress,
        nonce: FrameNonce,
        payload: Vec<u8>,
    ) -> Result<(), ()>;
}

impl<T> TxClient for Arc<T>
where
    T: TxClient,
{
    fn transmission_done(&self, nonce: FrameNonce) -> Result<(), ()> {
        return T::transmission_done(self.as_ref(), nonce);
    }

    fn transmission_successful(&self, recipient: LoRaAddress, nonce: FrameNonce) -> Result<(), ()> {
        return T::transmission_successful(self.as_ref(), recipient, nonce);
    }

    fn transmission_failed(
        &self,
        recipient: LoRaAddress,
        nonce: FrameNonce,
        payload: Vec<u8>,
    ) -> Result<(), ()> {
        return T::transmission_failed(self.as_ref(), recipient, nonce, payload);
    }
}

/// Reception client, acts like a callback on reception of radio messages.
///
/// The inner functiosn will be called when the device will receive new payloads.
pub trait RxClient {
    /// Device has received the given message.
    fn receive(&self, sender: LoRaAddress, payload: Vec<u8>, nonce: FrameNonce) -> Result<(), ()>;
}

impl<T> RxClient for Arc<T>
where
    T: RxClient,
{
    fn receive(&self, sender: LoRaAddress, payload: Vec<u8>, nonce: FrameNonce) -> Result<(), ()> {
        return T::receive(self.as_ref(), sender, payload, nonce);
    }
}
