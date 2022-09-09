use crate::{LoRaAddress, LoRaDestination};

/// Device trait represents a unit system that can receive and send messages using
/// some complex features like Adaptive-Rate-Power-Rate, Acknowledgment or Packet Aggregation.
#[derive(thiserror::Error, Debug)]
pub enum QueueError<T> {
    #[error("Internal device error. Error not linked to queue being full, no need to transmit.")]
    DeviceError(#[from] T),
    #[error("Queue is full. Transmit first to clear the queue and try again.")]
    QueueFullError(#[from] T),
}

pub trait Device<'a> {
    type DeviceError;

    /// Register the new transmission client which will recieve packet acknowledgment and
    /// transmission completion signal.
    fn set_transmit_client(&mut self, client: &'a dyn TxClient);

    /// Register the new reciever client which will be call for every packet received matching
    // the device address.
    fn set_receive_client(&mut self, client: &'a dyn RxClient);

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
    fn transmit(&mut self);

    /// Put the device in listening mode, waiting to recieve new packets on its address.
    fn start_reception(&mut self);

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
    fn send_done(&self);
}

/// Device Rx Client
///
/// Trait that act like a callback: this function will be called when the device will receive new payloads.
pub trait RxClient {
    /// Device has received the given message.
    fn receive(&self);
}
