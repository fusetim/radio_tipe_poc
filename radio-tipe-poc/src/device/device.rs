use crate::{LoRaAddress, LoRaDestination};
use crate::device::frame::FrameNonce;

/// Device trait represents a unit system that can receive and send messages using
/// some complex features like Adaptive-Rate-Power-Rate, Acknowledgment or Packet Aggregation.
#[derive(thiserror::Error, Debug)]
pub enum QueueError<T> {
    #[error("Internal device error. Error not linked to queue being full, no need to transmit.")]
    DeviceError(#[from] T),
    #[error("Queue is full. Transmit first to clear the queue and try again.")]
    QueueFullError(#[source] T),
}

/// Radio Device trait, representation for a specific device implementing the protocol.
///
/// TODO: Give default implementation for most of the inner method when they are not related to
/// a specifi radio implementation.
///
/// TODO: Implement a Mock device using the MockRadio provided by the radio crate.
pub trait Device<'a> {
    type DeviceError;

    /// Register the new transmission client which will recieve packet acknowledgment and
    /// transmission completion signal.
    fn set_transmit_client(&mut self, client: &'a mut dyn TxClient);

    /// Register the new reciever client which will be call for every packet received matching
    // the device address.
    fn set_receive_client(&mut self, client: &'a mut dyn RxClient);

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
    /// Periodical check need to be made with [[Device::check_reception]] to poll internal radio state
    /// and retrieve the received message by the physical device.
    fn start_reception(&mut self) -> Result<(), Self::DeviceError>;

    /// Check reception of messages by the physical radio.
    ///
    /// Periodical check need to be made with this method to poll internal radio state
    /// and retrieve the received message by the physical device.
    ///
    /// Note that this method can fail if the physical radio is not in reception mode (you should use
    /// [[Device::start_reception]] for that). Ypu might check this mode by using [[Device::is_listening]].
    fn check_reception(&mut self) -> Result<bool, Self::DeviceError>;

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
}

/// Device Tx Client
///
/// Trait that act like a callback: device will call this function to acknowledge completion and/or reception
/// of a previously queued payload.
pub trait TxClient {
    /// Device acknowledgment of transmission completed
    fn send_done(&mut self, nonce: FrameNonce) -> Result<(),()>;
}

/// Device Rx Client
///
/// Trait that act like a callback: this function will be called when the device will receive new payloads.
pub trait RxClient {
    /// Device has received the given message.
    fn receive(&mut self, sender: LoRaAddress, payload: Vec<u8>, nonce: FrameNonce)-> Result<(),()>;
}
