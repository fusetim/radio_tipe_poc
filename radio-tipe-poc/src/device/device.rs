use crate::LoRaAddress;

pub trait Device<'a> {
    type DeviceError;

    fn set_transmit_client(&mut self, client: &'a dyn TxClient);
    fn set_receive_client(&mut self, client: &'a dyn RxClient);
    fn set_address(&mut self, address: LoRaAddress);
    fn get_address(&self) -> &LoRaAddress;
    fn is_transmitting(&mut self) -> Result<bool, Self::DeviceError>;
    fn is_receiving(&mut self) -> Result<bool, Self::DeviceError>;
    fn transmit();
    fn start_reception();
}

pub trait TxClient {
    fn send_done(&self, );
}

pub trait RxClient {
    fn receive(&self, );
}