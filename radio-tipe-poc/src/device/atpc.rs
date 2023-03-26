use crate::device::frame::FrameNonce;
use std::time::Instant;

use bitflags::bitflags;

/// Control Model represents a model of the RSSI on the receiver end when the transmitter uses a particular 
/// Transmission Power (Transmission Level).
/// 
/// This model uses the following approximation: RSSI = a * TP + b for a particular ControlModel(a,b).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ControlModel(i16, i16);

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NeighborStatus {
    Idle,
    Initializing,
    Runtime,
}

bitflags! {
    pub struct TPStatus : u32 {

    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NeighborModel {
    pub node_address: u16, 
    pub status: NeighborStatus,
    pub control_model: ControlModel,
    pub rssi: Vec<i16>,
    pub delta_rssi: i16,
    pub tp_status: TPStatus,
}

impl NeighborModel {
    pub fn new(node_address: u16) -> Self {
        NeighborModel {
            node_address,
            status: NeighborStatus::Initializing,
            control_model: ControlModel(0,0),
            rssi: Vec::new(),
            delta_rssi: 0,
            tp_status: TPStatus::empty(),
        }
    }
}



pub trait ATPC {

    // TODO ATPC : Need a way to send the initialization beacons

    fn register_neighbor(&mut self, neighbor_addr: u16) -> bool;

    fn unregister_neighbor(&mut self, neighbor_addr: u16) -> bool;

    fn get_tx_power(&self, neighbor_addr: u16) -> u8;

    //fn handle_beacon(&mut self, beacon: impl Into<Beacon>) -> Result<(), ()>;

    fn report_successful_reception(&mut self, neighbor_addr: u16, drssi: i16);

    fn report_failed_reception(&mut self, neighbor_addr: u16);

}

pub struct DefaultATPC {
    pub neighbors: Vec<NeighborModel>,
    pub transmission_powers: Vec<u8>,
    pub default_tp: u8,
    pub upper_rssi: i16,
    pub lower_rssi: i16,
    pub target_rssi: i16,
    pub last_beacon: (FrameNonce, Instant),
}

impl DefaultATPC {
    pub fn new(transmission_powers: Vec<u8>, default_tp: impl Into<u8>, target_rssi: i16, upper_rssi: i16, lower_rssi: i16) -> Self {
        let default_tp_ = default_tp.into();
        assert!(default_tp_ < transmission_powers.len() as u8);
        Self {
            neighbors: Vec::new(),
            transmission_powers,
            default_tp: default_tp_,
            upper_rssi,
            lower_rssi,
            target_rssi,
            last_beacon: (0, Instant::now()),
        }
    }

    pub fn update_neighbor_model(&mut self, neighbor_addr: u16, delta: i16) {
        if let Some((i, _)) = self.neighbors.iter().enumerate().find(|(_, neigh)| neigh.node_address == neighbor_addr) {
            let tp = self.get_tx_power(neighbor_addr);
            if (delta > 0 && (tp as usize) < self.transmission_powers.len()-1) || (delta < 0 && tp > 0) {
                self.neighbors[i].control_model.1 -= delta;
            } 
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
        if let Some((i, _)) = self.neighbors.iter().enumerate().find(|(_, neigh)| neigh.node_address == neighbor_addr) {
            let _ = self.neighbors.swap_remove(i);
            true
        } else {
            false
        }
    }

    fn get_tx_power(&self, neighbor_addr: u16) -> u8 {
        if let Some(neigh) = self.neighbors.iter().find(|neigh| neigh.node_address == neighbor_addr) {
            let tp_target = (self.lower_rssi - neigh.control_model.1) / neigh.control_model.0;
            if let Some(tp) = self.transmission_powers.iter().find(|tp| (**tp as i16) >= tp_target) {
                return *tp;
            } else {
                return self.transmission_powers[self.transmission_powers.len() - 1];
            }
        }
        self.transmission_powers[self.default_tp as usize]
    }

    /*fn handle_beacon(&mut self, beacon: impl Into<Beacon>) -> Result<(), ()> {
        
    }*/

    fn report_successful_reception(&mut self, neighbor_addr: u16, drssi: i16) {
        self.update_neighbor_model(neighbor_addr, drssi);
    }

    fn report_failed_reception(&mut self, neighbor_addr: u16){
        self.update_neighbor_model(neighbor_addr, -30);
    }
}