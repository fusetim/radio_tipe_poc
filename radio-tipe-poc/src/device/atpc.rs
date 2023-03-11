use crate::device::frame::FrameNonce;
use std::time::Instant;

use bitflags::bitflags;

/// Control Model represents a model of the RSSI on the receiver end when the transmitter uses a particular 
/// Transmission Power (Transmission Level).
/// 
/// This model uses the following approximation: RSSI = a * TP + b for a particular ControlModel(a,b).
/// TODO/NOTE: RFM95W -> Absolute value of the RSSI in dBm, 0.5dB steps?
pub struct ControlModel(i16, i16);

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NeighborStatus {
    Idle,
    Initializing,
    Runtime,
}

bitflags! {
    struct TPStatus : u32 {

    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NeighborModel {
    pub node_address: u16, 
    pub status: NeighborStatus,
    pub control_model: ControlModel,
    pub rssi: i16,
    pub delta_rssi: i16,
    pub tp_status: TPStatus,
}

impl NeighborModel {
    pub fn new(node_address: u16) -> Self {
        NeighborModel {
            node_address,
            status: NeighborStatus::Initializing,
            control_model: ControlModel(0,0),
            rssi: 0,
            delta_rssi: 0,
            tp_status: TPStatus::empty(),
        }
    }
}



pub trait ATPC {

    fn register_neighbor(&mut self, neighbor_addr: u16) -> bool;

    fn unregister_neighbor(&mut self, neighbor_addr: u16) -> bool;

    fn get_tx_power(&self, neighbor_addr: u16) -> u8;

    fn handle_beacon(&mut self, beacon: impl Into<Beacon>) -> Result<(), ()>;

    fn report_successful_reception(&mut self, neighbor_addr: u16, rssi: f32);

    fn report_successful_transmission(&mut self, neighbor_addr: u16);

};

pub struct DefaultATPC {
    pub neighbors: Vec<NeighborModel>,
    pub transmission_powers: Vec<i16>,
    pub default_tp: u8,
    pub upper_rssi: i16,
    pub lower_rssi: i16,
    pub target_rssi: i16,
    pub last_beacon: (FrameNonce, Instant),
}

impl DefaultATPC {
    pub fn new(transmission_powers: Vec<i16>, default_tp: impl Into<usize>, target_rssi: i16, upper_rssi: i16, lower_rssi: i16) -> Self {
        assert!(default_tp.into() < transmission_powers.len());
        Self {
            neighbors: Vec::new(),
            transmission_powers,
            default_tp.into(),
            upper_rssi,
            lower_rssi,
            target_rssi,
            last_beacon: (0, Instant::now()),
        }
    }
}

impl ATPC for DefaultATPC {
    fn register_neighbor(&mut self, neighbor_addr: u16) -> bool {
        if let None = self.neighbors.iter().find(|neigh| neigh.node_address == neighbor_addr) {
            let neigh = NeighborModel::new(neighbor_addr);
            self.neighbors.push(neigh);
            true
        } else {
            false
        }
    }

    fn unregister_neighbor(&mut self, neighbor_addr: u16) -> bool {
        if let Some((n_, i)) = self.neighbors.iter().enumerate().find(|(neigh, _)| neigh.node_address == neighbor_addr) {
            let _ = self.neighbors.swap_remove(i);
            true
        } else {
            false
        }
    }

    fn get_tx_power(&self, neighbor_addr: u16) -> u8 {
        if let Some(neigh) = self.neighbors.iter().find(|neigh| neigh.node_address == neighbor_addr) {
            let tp_target = (neigh.control_model.0 * self.target_rssi + neigh.control_model.1) as u8;
            if let Some(tp) = self.transmission_powers.find(|tp| tp >= tp_target) {
                return tp;
            }
        }
        self.transmission_powers[self.default_tp as usize];
    }

    fn handle_beacon(&mut self, beacon: impl Into<Beacon>) -> Result<(), ()>;

    fn report_successful_transmission(&mut self, neighbor_addr: u16);
}