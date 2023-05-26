//! Adaptive Transmission Power Control interfaces and basic implementations.
//!
//! This module provides the public trait to implement an ATPC at the application level.
//! Moreover it provides two implementations, a naive implementation that basically disable
//! the ATPC and a [standard implementation](DefaultATPC) based on
//! [Shan Lin's work](https://www.cs.virginia.edu/~stankovic/psfiles/ATPC.pdf).
//!
//! ## Usages
//! Either just use a provided implementation and passed it to your [LoRaRadio](crate::radio::LoRaRadio).
//! ```rust,ignore
//! let atpc = radio_tipe_poc::atpc::TestingATPC::new(vec![10, 8, 6, 4, 2]);
//! let mut device = LoRaRadio::new(lora, &channels, atpc, -100, None, None, 0b0101_0011);
//! ```
//! Or implement your own ATPC by creating your structure who implement the [ATPC] trait.
use crate::frame::FrameNonce;
use crate::LoRaAddress;

use std::cmp::Ordering;
use std::num::NonZeroUsize;
use std::time::Duration;
use std::time::Instant;

use lru::LruCache;

/// Modelisation of the RSSI on the receiver end when the transmitter uses a particular
/// Transmission Power (Transmission Level).
///
/// This model uses the following approximation: `RSSI = a * TP + b` for a particular `ControlModel(a,b)`.
///
/// This model follows the design provided in [Shan Lin's work](https://www.cs.virginia.edu/~stankovic/psfiles/ATPC.pdf).
#[derive(Clone, PartialEq, Eq, Debug)]
struct ControlModel(i16, i16);

/// Status of a neighbor for the [DefaultATPC].
#[derive(Clone, PartialEq, Eq, Debug)]
enum NeighborStatus {
    /// This neighbor has not yet answered to our beacons (or partially). We currently have no
    /// information on the transmission power needed for this peer.
    Initializing,
    /// This neighbor has been fully initialized. Its control model is valid. It was successfully built
    /// with the answers from the peer to our beacons.
    Runtime,
}

/// Representation of a peer for the [DefaultATPC].
#[derive(Clone, Debug)]
struct NeighborModel {
    /// Address of this peer.
    pub node_address: LoRaAddress,
    /// Status of the peer for the ATPC.
    pub status: NeighborStatus,
    /// Dedicated control model for this particular node.
    pub control_model: ControlModel,
    /// RSSI responses for the various transmissions power levels.
    ///
    /// Those are calculated with the acknowledgments given by the peer. This includes
    /// the answers to our beacons.
    pub rssi: Vec<i16>,
}

impl Ord for NeighborModel {
    fn cmp(&self, other: &Self) -> Ordering {
        self.node_address.cmp(&other.node_address)
    }
}

impl PartialEq for NeighborModel {
    fn eq(&self, other: &Self) -> bool {
        self.node_address == other.node_address
    }
}
impl Eq for NeighborModel {}

impl PartialOrd for NeighborModel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl NeighborModel {
    /// Constructs a new instance of a neighbor model.
    ///
    /// Due to its implementation being separated from the [DefaultATPC],
    /// we need to pass the number of transmission power levels that are
    /// tracked by the ATPC.
    fn new(node_address: LoRaAddress, ntp: usize) -> Self {
        NeighborModel {
            node_address,
            status: NeighborStatus::Initializing,
            control_model: ControlModel(0, 0),
            rssi: vec![0; ntp],
        }
    }
}

/// Abstract representation of an Adaptable Transmission Power Control (ATPC).
///
/// This trait is an essential component of the [LoRaRadio](crate::device::radio::LoRaRadio).
/// This is this module who determine for each peer the needed transmission power to successfully
/// transmit a frame to a neighbor while helping reducing the energy consumption due to radio
/// transmission.
pub trait ATPC {
    /// Should the radio transmit beacons ? It is mostly determined by the time elapsed from the last
    /// transmission of beacons and the registration of unknown peers that are waiting for initialization.
    fn is_beacon_needed(&self) -> bool;

    /// Gives a list of transmission power to use to transmit the beacons.
    /// Those might or not be equal to the transmission powers given at construction of an ATPC.
    ///
    /// Note that this function might return an empty Vec if the ATPC does not implement beacon.
    fn get_beacon_powers(&self) -> Vec<i8>;

    /// Registers a beacon with its transmission power (index in the [get_beacon_powers](ATPC::get_beacon_powers))
    /// and its nonce.
    ///
    /// This ensures [report_successful_reception](ATPC::report_successful_reception) can correctly
    /// update the [ControlModel] of each neighbor.
    fn register_beacon(&mut self, tpi: usize, nonce: FrameNonce);

    /// Registers a neighbor. This indicates an interest by the radio to transmit data to this peer.
    ///
    /// This function might cause (if the peer is unknown) a transmission of beacons.
    fn register_neighbor(&mut self, neighbor_addr: LoRaAddress) -> bool;

    /// Unregisters a neighbor. It might force to forget this particular neighbor.
    fn unregister_neighbor(&mut self, neighbor_addr: LoRaAddress) -> bool;

    /// Calculates the needed transmission power for a particular neighbor.
    fn get_tx_power(&mut self, neighbor_addr: LoRaAddress) -> i8;

    /// Calculates the needed transmission power for a particular set of neighbors.
    fn get_min_tx_power(&mut self, mut neighbor_addrs: Vec<LoRaAddress>) -> (i8, Vec<LoRaAddress>) {
        // Minimal default implementation.
        let mut tx_power = 0;
        let mut should_update = Vec::new();
        neighbor_addrs.sort();
        for na in &neighbor_addrs {
            let tp = self.get_tx_power(*na);
            if tp > tx_power {
                tx_power = tp;
                should_update.clear();
                should_update.push(*na);
            } else if tp == tx_power {
                should_update.push(*na);
            }
        }
        if should_update.len() > 0 {
            return (tx_power, should_update);
        } else {
            return (0, neighbor_addrs);
        }
    }

    /// Reports the reception of an acknownledgment (maybe for a beacon) by a neighbor.
    ///
    /// This will update the [ControlModel] of this particular peer accordingly to the given
    /// `drssi` (Delta between the RSSI target and the received RSSI of this tranmission).
    fn report_successful_reception(
        &mut self,
        neighbor_addr: LoRaAddress,
        nonce: FrameNonce,
        drssi: i16,
    );

    /// Reports the lack of acknownledgment (maybe for a beacon) by a neighbor.
    ///
    /// This will update the [ControlModel] of this particular peer accordingly
    fn report_failed_reception(&mut self, neighbor_addr: LoRaAddress);
}

/// Default implementation of the ATPC, based on [Shan Lin's work](https://www.cs.virginia.edu/~stankovic/psfiles/ATPC.pdf).
///
/// It provides an efficient implementation that can adapt to its surrounding and with a small cost
/// of only three beacon tranmissions per day. Moreover the design is pretty simple and offer
/// good results in different real case scenarios.
pub struct DefaultATPC {
    /// LRU Cache to remember the parameters associated with the most recent neighbors.
    neighbors: LruCache<LoRaAddress, NeighborModel>,
    /// The transmission powers usable by the ATPC (and the radio).
    transmission_powers: Vec<i8>,
    /// The default transmission power (the index of it in `transmission_powers`) that will
    /// be use if a node is unknown or still initializing.
    default_tp: u8,
    /// The minimal RSSI threashold that the radio will consider acceptable.
    lower_rssi: i16,
    /// Delay between beacon broadcasting.
    ///
    /// 8h seems a good value.
    beacon_delay: Duration,
    /// The latest beacons transmitted as a nonce-transmission power level value.
    beacons: LruCache<FrameNonce, u8>,
    /// Last time a beacon was transmitted.
    last_beacon: Instant,
}

impl DefaultATPC {
    /// Builds a new instance of the Default ATPC.
    pub fn new(
        transmission_powers: Vec<i8>,
        default_tp: impl Into<u8>,
        lower_rssi: i16,
        beacon_delay: Duration,
    ) -> Self {
        let default_tp_ = default_tp.into();
        let tp_len = transmission_powers.len();
        assert!(default_tp_ < tp_len as u8);
        Self {
            neighbors: LruCache::new(NonZeroUsize::new(128).unwrap()),
            transmission_powers,
            default_tp: default_tp_,
            lower_rssi,
            beacons: LruCache::new(NonZeroUsize::new(tp_len + 1).unwrap()),
            last_beacon: Instant::now(),
            beacon_delay,
        }
    }

    /// Rebuilds the [ControlModel] of a specific neighbor.
    ///
    /// Mostly used to update a node following a beacon acknowledgment.
    fn rebuid_neighbor_model(&mut self, neighbor_addr: LoRaAddress) {
        if let Some(neigh) = self.neighbors.get_mut(&neighbor_addr) {
            let n = self.transmission_powers.len();
            let sum_tp: f32 = self
                .transmission_powers
                .iter()
                .fold(0.0, |acc, x| acc + (*x as f32));
            let sum_rssi: f32 = neigh.rssi.iter().fold(0.0, |acc, x| acc + (*x as f32));
            let sum_tp_rssi: f32 = (0..self.transmission_powers.len())
                .into_iter()
                .fold(0.0, |acc, i| {
                    acc + (self.transmission_powers[i] as f32) * (neigh.rssi[i] as f32)
                });
            let denominator: f32 = (n as f32)
                * self
                    .transmission_powers
                    .iter()
                    .fold(0.0, |acc, x| acc + (*x as f32) * (*x as f32))
                + sum_tp * sum_tp;

            neigh.control_model.0 =
                (((sum_rssi * sum_tp * sum_tp) - (sum_tp * sum_tp_rssi)) / denominator) as i16;
            neigh.control_model.1 =
                ((((n as f32) * sum_tp_rssi) - (sum_tp * sum_rssi)) / denominator) as i16;
            neigh.status = NeighborStatus::Runtime;
        }
    }

    /// Updates the [ControlModel] of a specific neighbor.
    ///
    /// Mostly used to update a node following a successful/failed transmission.
    fn update_neighbor_model(&mut self, neighbor_addr: LoRaAddress, delta: i16) {
        let tp = self.get_tx_power(neighbor_addr);
        if let Some(neigh) = self.neighbors.get_mut(&neighbor_addr) {
            if (delta > 0 && tp < self.transmission_powers[self.transmission_powers.len() - 1])
                || (delta < 0 && tp > self.transmission_powers[0])
            {
                neigh.control_model.1 -= delta;
            }
        }
    }

    /// Calculates the transmission power needed for a particular node/neighbor.
    fn calc_node_tp(&mut self, neighbor_addr: LoRaAddress) -> i8 {
        let neigh = self
            .neighbors
            .get(&neighbor_addr)
            .expect("calculating TP for an inexistant neighbor.");
        let tp_target = (self.lower_rssi - neigh.control_model.1) / neigh.control_model.0;
        if let Some(tp) = self
            .transmission_powers
            .iter()
            .find(|tp| (**tp as i16) >= tp_target)
        {
            return *tp;
        } else {
            return self.transmission_powers[self.transmission_powers.len() - 1];
        }
    }
}

impl ATPC for DefaultATPC {
    fn is_beacon_needed(&self) -> bool {
        return self.last_beacon.elapsed() > self.beacon_delay
            || self
                .neighbors
                .iter()
                .find(|(_, n)| n.status == NeighborStatus::Initializing)
                .is_some();
    }

    fn get_beacon_powers(&self) -> Vec<i8> {
        return self.transmission_powers.clone();
    }

    fn register_beacon(&mut self, tpi: usize, nonce: FrameNonce) {
        self.last_beacon = Instant::now();
        self.beacons.push(nonce, tpi as u8);
    }

    fn register_neighbor(&mut self, neighbor_addr: LoRaAddress) -> bool {
        // We should assure the unicity of the neighbors in the list.
        if let None = self.neighbors.get(&neighbor_addr) {
            let neigh = NeighborModel::new(neighbor_addr, self.transmission_powers.len());
            self.neighbors.push(neighbor_addr, neigh);
            true
        } else {
            false
        }
    }

    fn unregister_neighbor(&mut self, neighbor_addr: LoRaAddress) -> bool {
        return self.neighbors.pop_entry(&neighbor_addr).is_some();
    }

    fn get_tx_power(&mut self, neighbor_addr: LoRaAddress) -> i8 {
        if self.neighbors.contains(&neighbor_addr) {
            return self.calc_node_tp(neighbor_addr);
        }
        self.transmission_powers[self.default_tp as usize]
    }

    fn get_min_tx_power(&mut self, mut neighbor_addrs: Vec<LoRaAddress>) -> (i8, Vec<LoRaAddress>) {
        let mut tx_power = None;
        let mut should_update = Vec::new();
        neighbor_addrs.sort();
        for na in &neighbor_addrs {
            let tp = self.get_tx_power(*na);
            if tx_power.is_none() || tp == tx_power.unwrap() {
                should_update.push(*na);
            } else if tp > tx_power.unwrap() {
                tx_power = Some(tp);
                should_update.clear();
                should_update.push(*na);
            }
        }
        if let Some(tx_power) = tx_power {
            (tx_power, should_update)
        } else {
            (
                self.transmission_powers[self.default_tp as usize],
                neighbor_addrs,
            )
        }
    }

    fn report_successful_reception(
        &mut self,
        neighbor_addr: LoRaAddress,
        nonce: FrameNonce,
        drssi: i16,
    ) {
        if let Some(tpi) = self.beacons.get(&nonce) {
            if let Some(neigh) = self.neighbors.get_mut(&neighbor_addr) {
                neigh.rssi[*tpi as usize] = drssi;
                self.rebuid_neighbor_model(neighbor_addr);
            }
        } else {
            self.update_neighbor_model(neighbor_addr, drssi);
        }
    }

    fn report_failed_reception(&mut self, neighbor_addr: LoRaAddress) {
        self.update_neighbor_model(neighbor_addr, -30);
    }
}

/// Testing implementation.
///
/// Provides an implementation that cycles all its transmission powers across each transmission.
/// Moreover it does not implement beacons, and most of its operations are NO-OP.
pub struct TestingATPC {
    /// The transmission powers usable by the ATPC (and the radio).
    transmission_powers: Vec<i8>,
    /// Literally a counter of each transmission.
    counter: usize,
}

impl TestingATPC {
    /// Builds a new instance of a Testing ATPC.
    pub fn new(transmission_powers: Vec<i8>) -> Self {
        Self {
            transmission_powers,
            counter: 0,
        }
    }
}

impl ATPC for TestingATPC {
    fn is_beacon_needed(&self) -> bool {
        false
    }

    fn get_beacon_powers(&self) -> Vec<i8> {
        return vec![];
    }

    fn register_beacon(&mut self, _tpi: usize, _nonce: FrameNonce) {
        // NO-OP
    }

    fn register_neighbor(&mut self, _neighbor_addr: LoRaAddress) -> bool {
        // NO OP
        true
    }

    fn unregister_neighbor(&mut self, _neighbor_addr: LoRaAddress) -> bool {
        // NO OP
        true
    }

    fn get_tx_power(&mut self, _neighbor_addr: LoRaAddress) -> i8 {
        let tp = self.transmission_powers[self.counter];
        let len = self.transmission_powers.len();
        self.counter = (self.counter + 1) % len;
        return tp;
    }

    fn get_min_tx_power(&mut self, neighbor_addrs: Vec<LoRaAddress>) -> (i8, Vec<LoRaAddress>) {
        return (self.get_tx_power(*&neighbor_addrs[0]), neighbor_addrs);
    }

    fn report_successful_reception(
        &mut self,
        _neighbor_addr: LoRaAddress,
        _nonce: FrameNonce,
        _drssi: i16,
    ) {
        // NO OP
    }

    fn report_failed_reception(&mut self, _neighbor_addr: LoRaAddress) {
        // NO OP
    }
}
