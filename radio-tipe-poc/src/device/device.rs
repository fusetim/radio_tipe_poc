pub trait Device<'a> {
    type DeviceError;

    pub fn set_transmit_client(&mut self, client: &'a dyn TxClient);
    pub fn set_receive_client(&mut self, client: &'a dyn RxClient);
    pub fn set_address(&mut self, address: LoRaAddress);
    pub fn get_address(&self) -> &LoRaAddress;
    pub fn is_transmitting(&mut self) -> Result<bool, Self::DeviceError>;
    pub fn is_receiving(&mut self) -> Result<bool, Self::DeviceError>;
    pub fn transmit();
    pub fn start_reception();
}

pub trait TxClient {
    fn send_done(&self, );
}

pub trait RxClient {
    fn receive(&self, );
}