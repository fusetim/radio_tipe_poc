
pub enum FrameType {
    Message = 0,
    Acknowledgment = 1,
    RelayAnnouncement = 2,
    RelayAnnouncementAcknowledgment = 3,
    RelayMessage = 4, // Use the same Frame template as Message, just a different FrameType.
    RelayAcknowledgment = 5, // Use the same Frame template as Acknowledgment, just a different FrameType.
    BroadcastCheckSignal = 6,
    BroadcastCheckSignalReply = 7,
}

/// Trait to calculate size on frame for every component on frame.
pub trait FrameSize {
    /// Calculate compenent size on frame (meaning encoded) in bytes.
    fn size(&self) -> usize;
}

pub type FrameNonce = u64;
const FRAME_NONCE_SIZE : usize = 8;

/// Radio header representation.
#[derive(Clone, Debug)]
pub struct RadioHeaders {
    /// Number of Recipients and frames of this transmission.
    pub rec_n_frames: InfoHeader,
    /// Inner recipient headers, representing the recipient address and payload associations.
    pub recipients: RecipientHeader,
    /// Number of payloads (limited to 16).
    pub payloads: u8,
    /// Sender address
    pub sender: AddressHeader,
    //stats: (),
    /// TODO / PROPOSAL / SECURITY : use timestamp + 16 random bits
    /// 
    /// Nonce MUST follow a total order.
    pub nonce: FrameNonce,
    // TODO / PROPOSAL / SECURITY : add frame signature (64 bytes for Ed25519)
    // pub signature: [u8; 64];
}

/// Full representation of a Radio frame with headers and payloads.
#[derive(Clone, Debug)]
pub struct RadioFrameWithHeaders {
    /// Frame headers
    pub headers: RadioHeaders,
    /// Frame payloads
    pub payloads: Vec<Payload>,
}

pub type RadioFrame = Vec<Payload>;

/// Compact representation of recipient number and frame number.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct InfoHeader(u8);

/// Compact representation of recipient (with payload association) and acknowledgment handling.
#[derive(Clone, Debug)]
pub enum RecipientHeader {
    /// Direct message, there is only one recipient and all of the message is for it.
    Direct(AddressHeader),
    /// Group message, representation for packet aggregation and/or payload with multiple recipient.
    Group(Vec<(AddressHeader, PayloadFlag)>),
}

/// Compact representation of an LoRa address (and acknowledgment).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AddressHeader(u16);

/// Compact representation of recipient-payload association.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PayloadFlag(u16);

pub(crate) type Payload = Vec<u8>;

impl InfoHeader {
    pub fn new(recipients: u8, frames: u8) -> Self {
        let mut inner = 0u8;
        inner += (recipients << 4) & 0b11110000;
        inner += frames & 0b00001111;
        return InfoHeader(inner);
    }

    pub fn set_recipients(&mut self, recipients: u8) -> Self {
        let mut inner = 0u8;
        inner += (recipients << 4) & 0b11110000;
        inner += self.0 & 0b00001111;
        self.0 = inner;
        *self
    }

    pub fn set_frames(&mut self, frames: u8) -> Self {
        let mut inner = 0u8;
        inner += self.0 & 0b11110000;
        inner += frames & 0b00001111;
        self.0 = inner;
        *self
    }

    pub fn get_recipients(&self) -> u8 {
        ((self.0) >> 4) & 0b00001111
    }

    pub fn get_frames(&self) -> u8 {
        self.0 & 0b00001111
    }
}

pub const GLOBAL_ACKNOWLEDGMENT: u16 = 0b1111_1111_1111_1111;
pub const GLOBAL_NO_ACKNOWLEDGMENT: u16 = 0b0111_1111_1111_1111;
pub const ADDRESS_BITMASK: u16 = 0b0111_1111_1111_1111;
pub const ACKNOWLEDGMENT_BITMASK: u16 = 0b1000_0000_0000_0000;

impl AddressHeader {
    pub fn new(addr: u16, ack: bool) -> Self {
        let mut inner = addr & ADDRESS_BITMASK;
        if ack {
            inner |= ACKNOWLEDGMENT_BITMASK
        };
        Self(inner)
    }

    pub fn new_global(ack: bool) -> Self {
        if ack {
            Self(GLOBAL_ACKNOWLEDGMENT)
        } else {
            Self(GLOBAL_NO_ACKNOWLEDGMENT)
        }
    }

    pub fn set_acknowledgment(&mut self, ack: bool) -> Self {
        let mut inner = self.0 & ADDRESS_BITMASK;
        if ack {
            inner |= ACKNOWLEDGMENT_BITMASK;
        };
        self.0 = inner;
        *self
    }

    pub fn set_address(&mut self, addr: u16) -> Self {
        let mut inner = addr & ADDRESS_BITMASK;
        inner |= self.0 & ACKNOWLEDGMENT_BITMASK;
        self.0 = inner;
        *self
    }

    pub fn set_global(&mut self) -> Self {
        self.set_address(GLOBAL_NO_ACKNOWLEDGMENT)
    }

    pub fn get_acknowledgment(&self) -> bool {
        (self.0 & ACKNOWLEDGMENT_BITMASK) == ACKNOWLEDGMENT_BITMASK
    }

    pub fn get_address(&self) -> u16 {
        self.0 & ADDRESS_BITMASK
    }

    pub fn is_global(&self) -> bool {
        (self.get_address() & ADDRESS_BITMASK) == GLOBAL_NO_ACKNOWLEDGMENT
    }
}

impl PayloadFlag {
    /// Message ID should be included in 0..16 (16 excluded) in the slice.
    pub fn new(message_ids: &[u8]) -> Self {
        let mut inner = 0u16;
        for id in message_ids {
            inner |= 1 << (id);
        }
        Self(inner)
    }

    pub fn push(&mut self, id: u8) {
        self.0 |= 1 << id;
    }

    pub fn to_message_ids(&self) -> Vec<u8> {
        let mut ids = Vec::new();
        for i in 0..16 {
            /*if ((self.0 >> i) & 1) == 1 {
                ids.push(i);
            }*/
            if ((self.0 & (1 << i)) > 0) {
                ids.push(i);
            }
        }
        ids
    }
}

impl From<u16> for PayloadFlag {
    fn from(inner: u16) -> Self {
        PayloadFlag(inner)
    }
}

impl Into<u16> for PayloadFlag {
    fn into(self) -> u16 {
        self.0
    }
}

impl From<u16> for AddressHeader {
    fn from(inner: u16) -> Self {
        AddressHeader(inner)
    }
}

impl Into<u16> for AddressHeader {
    fn into(self) -> u16 {
        self.0
    }
}

impl From<u8> for InfoHeader {
    fn from(inner: u8) -> Self {
        InfoHeader(inner)
    }
}

impl Into<u8> for InfoHeader {
    fn into(self) -> u8 {
        self.0
    }
}

impl<'a> RecipientHeader {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        match self {
            RecipientHeader::Direct(addr) => {
                bytes.push(1);
                let addr_raw: u16 = (*addr).into();
                bytes.append(&mut addr_raw.to_be_bytes().to_vec());
            }
            RecipientHeader::Group(addrs) => {
                bytes.push(addrs.len() as u8);
                for (a, pf) in addrs {
                    let a_raw: u16 = (*a).into();
                    let pf_raw: u16 = (*pf).into();
                    bytes.append(&mut a_raw.to_be_bytes().to_vec());
                    bytes.append(&mut pf_raw.to_be_bytes().to_vec());
                }
            }
        }
        bytes
    }
    pub fn try_from_bytes(bytes: &'a [u8]) -> Result<(Self, usize), FrameError> {
        if bytes.len() < 1 {
            return Err(FrameError::InvalidHeader {
                context: Some(format!("Recipient header is too small (0 byte).")),
            });
        };
        match bytes[0] {
            0 => Err(FrameError::InvalidHeader {
                context: Some(format!("Recipient header with 0 recipient.")),
            }),
            1 => {
                if bytes.len() < 3 {
                    return Err(FrameError::InvalidHeader {
                        context: Some(format!("Recipient header is too small for a Direct trame.")),
                    });
                };
                let mut addr_raw = [0u8; 2];
                addr_raw.copy_from_slice(&bytes[1..3]);
                let addr = AddressHeader::from(u16::from_be_bytes(addr_raw));
                Ok((RecipientHeader::Direct(addr), 3))
            }
            2..=16 => {
                if bytes.len() < 1 + 4 * bytes[0] as usize {
                    return Err(FrameError::InvalidHeader{ context: Some(format!("Recipient header is too small for a Group trame ({} recipients and only {} bytes).", bytes[0], bytes.len()))});
                };
                let mut addrs = Vec::new();
                for i in 0..(bytes[0] as usize) {
                    let mut addr_raw = [0u8; 2];
                    addr_raw
                        .copy_from_slice(&bytes[((1 + i * 4) as usize)..((3 + i * 4) as usize)]);
                    let addr = AddressHeader::from(u16::from_be_bytes(addr_raw));
                    let mut pf_raw = [0u8; 2];
                    pf_raw.copy_from_slice(&bytes[((3 + i * 4) as usize)..((5 + i * 4) as usize)]);
                    let pf = PayloadFlag::from(u16::from_be_bytes(addr_raw));
                    addrs.push((addr, pf));
                }
                Ok((
                    RecipientHeader::Group(addrs),
                    5 + ((bytes[0] as usize) - 1) * 4,
                ))
            }
            n => Err(FrameError::InvalidHeader {
                context: Some(format!(
                    "Recipient header with too many recipients ({}).",
                    n
                )),
            }),
        }
    }
}

impl RadioHeaders {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let rnt_raw: u8 = self.rec_n_frames.into();
        bytes.push(rnt_raw.to_be());
        bytes.append(&mut self.recipients.to_bytes());
        let src_raw: u16 = self.sender.into();
        bytes.append(&mut src_raw.to_be_bytes().to_vec());
        bytes.push(self.payloads.to_be());
        let nonce_raw: u64 = self.nonce.into();
        bytes.append(&mut nonce_raw.to_be_bytes().to_vec());
        bytes
    }

    pub fn try_from_bytes<'a>(bytes: &'a [u8]) -> Result<(Self, usize), FrameError> {
        if bytes.len() < 1 {
            return Err(FrameError::InvalidHeader {
                context: Some(format!("Radio header too small! (0 bytes)")),
            });
        };
        let rec_n_frames = InfoHeader::from(u8::from_be(bytes[0]));
        let (recipients, read) = RecipientHeader::try_from_bytes(&bytes[1..])?;
        if bytes.len() < read + 3 {
            return Err(FrameError::InvalidHeader {
                context: Some(format!("Badly formatted frame, missing source address!")),
            });
        };
        let mut src_raw = [0u8; 2];
        src_raw.copy_from_slice(&bytes[(read + 1)..(read + 3)]);
        let sender = AddressHeader::from(u16::from_be_bytes(src_raw));
        if bytes.len() < read + 4 {
            return Err(FrameError::InvalidHeader {
                context: Some(format!(
                    "Badly formatted frame, missing number of payloads!"
                )),
            });
        };
        let payloads = u8::from_be(bytes[read + 3]);
        
        let mut nonce_raw = [0u8; FRAME_NONCE_SIZE];
        nonce_raw.copy_from_slice(&bytes[(read + 4)..(read + 4 + FRAME_NONCE_SIZE)]);
        let nonce = u64::from_be_bytes(nonce_raw);
        if bytes.len() < read + 8 {
            return Err(FrameError::InvalidHeader {
                context: Some(format!("Badly formatted frame, missing nonce!")),
            });
        };
        Ok((
            RadioHeaders {
                rec_n_frames,
                recipients,
                payloads,
                sender,
                nonce,
            },
            read + 8,
        ))
    }
}

impl RadioFrameWithHeaders {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.headers.to_bytes();
        assert!(
            self.headers.payloads == self.payloads.len() as u8,
            "Number of payload is invalid, not equal number in header and in frame."
        );
        for pl in &self.payloads {
            bytes.append(&mut (pl.len() as u16).to_be_bytes().to_vec());
            bytes.append(&mut pl.into_iter().map(|b| b.to_be()).collect());
        }
        bytes
    }

    pub fn try_from_bytes<'a>(bytes: &'a [u8]) -> Result<(Self, usize), FrameError> {
        let (headers, read) = RadioHeaders::try_from_bytes(bytes)?;
        let mut cursor = read;
        let mut payloads = Vec::new();
        for _i in 0..(headers.payloads as usize) {
            if bytes.len() < cursor {
                return Err(FrameError::InvalidHeader {
                    context: Some(format!("Fail to read payload length at byte {}!", cursor)),
                });
            };
            let mut len_raw = [0u8; 2];
            len_raw.copy_from_slice(&bytes[cursor..(cursor + 2)]);
            let len = u16::from_be_bytes(len_raw) as usize;
            if bytes.len() < cursor + len {
                return Err(FrameError::InvalidHeader {
                    context: Some(format!("Fail to read payload at byte {}!", cursor + len)),
                });
            };
            let payload: Vec<u8> = bytes[(cursor + 2)..(cursor + 2 + len)]
                .iter()
                .map(|b| u8::from_be(*b))
                .collect();
            cursor = cursor + 2 + len;
            payloads.push(payload);
        }
        Ok((RadioFrameWithHeaders { headers, payloads }, cursor))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum FrameError {
    #[error("Invalid header. Context: {}", .context.as_ref().unwrap_or(&"<none>".to_owned()))]
    InvalidHeader { context: Option<String> },

    #[error("Unknown frame error. Context: {}", context)]
    Unknown { context: String },
}

impl FrameSize for u8 {
    fn size(&self) -> usize {
        1
    }
}

impl FrameSize for u16 {
    fn size(&self) -> usize {
        2
    }
}

impl FrameSize for InfoHeader {
    fn size(&self) -> usize {
        1
    }
}

impl FrameSize for PayloadFlag {
    fn size(&self) -> usize {
        2
    }
}

impl FrameSize for AddressHeader {
    fn size(&self) -> usize {
        2
    }
}

impl FrameSize for Payload {
    fn size(&self) -> usize {
        2 + self.len()
    }
}

impl FrameSize for Vec<Payload> {
    fn size(&self) -> usize {
        self.into_iter().fold(1, |acc, p| acc + p.size())
    }
}

impl FrameSize for RadioHeaders {
    fn size(&self) -> usize {
        self.rec_n_frames.size()
            + self.recipients.size()
            + self.payloads.size()
            + self.sender.size()
            + FRAME_NONCE_SIZE
    }
}

impl FrameSize for RadioFrameWithHeaders {
    fn size(&self) -> usize {
        self.headers.size() + self.payloads.size()
    }
}

impl FrameSize for RecipientHeader {
    fn size(&self) -> usize {
        match self {
            RecipientHeader::Direct(addr) => addr.size(),
            RecipientHeader::Group(addrs) => addrs
                .into_iter()
                .fold(0, |acc, (addr, pf)| acc + addr.size() + pf.size()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_encode_info_header() {
        let ih1 = InfoHeader::new(0, 0);
        assert!(ih1.0 == 0b0000_0000, "Failed to initialize an empty InfoHeader, got: {:b}, expect {:b}", ih1.0, 0b0000_0000);

        let ih2 = InfoHeader::new(15, 0);
        assert!(ih2.0 == 0b1111_0000, "Failed to initialize an InfoHeader with 15 recipients, got: {:b}, expect {:b}", ih2.0, 0b1111_0000);

        let ih3 = InfoHeader::new(0,5);
        assert!(ih3.0 == 0b0000_0101, "Failed to initialize an InfoHeader with 5 frames, got: {:b}, expect {:b}", ih3.0, 0b0000_0101);

        let ih4 = InfoHeader::new(12, 3);
        assert!(ih4.0 == 0b1100_0011, "Failed to initialize an InfoHeader with 12 recipients and 3 frames, got: {:b}, expect {:b}", ih4.0, 0b1100_0011);

        let mut ih5 = ih1.clone();
        ih5.set_recipients(12);
        assert!(ih5.0 == 0b1100_0000, "Failed to edit an InfoHeader to 12 recipients, got: {:b}, expect {:b}", ih5.0, 0b1100_0000);
        ih5.set_frames(3);
        assert!(ih5.0 == 0b1100_0011, "Failed to edit an InfoHeader to 12 recipients and 3 frames, got: {:b}, expect {:b}", ih5.0, 0b1100_0011);
        ih5.set_recipients(0);
        assert!(ih5.0 == 0b0000_0011, "Failed to edit an InfoHeader to 0 recipients and 3 frames, got: {:b}, expect {:b}", ih5.0, 0b0000_0011);
    }

    #[test]
    fn frame_encode_global_address_header() {
        let ah1 = AddressHeader::new_global(true);
        assert_eq!(ah1.0, GLOBAL_ACKNOWLEDGMENT);
        let ah2 = AddressHeader::new_global(false);
        assert_eq!(ah2.0, GLOBAL_NO_ACKNOWLEDGMENT);
        assert!(ah1.is_global(), "ah1 is not recognized as a global address!");
        assert!(ah2.is_global(), "ah2 is not recognized as a global address!");

        assert!(ah1.get_acknowledgment(), "ah1 do not require acknowledgment while it does.");
        assert!(!ah2.get_acknowledgment(), "ah2 requires an acknowledgment while it does not!");
    }

    #[test]
    fn frame_encode_address_header() {
        let ah1 = AddressHeader::new(0b0011111111111111, false);
        let ah2 = AddressHeader::new(0b1011111111111111, false);
        let ah3 = AddressHeader::new(0b0011111111111111, true);
        assert_eq!(ah1.0, ah2.0, "AddressHeader should ignore acknowledgment flag on build!");
        
        assert!(!ah1.is_global(), "ah1 is recognized as a global address!");
        assert!(!ah2.is_global(), "ah2 is recognized as a global address!");
        assert!(!ah3.is_global(), "ah3 is recognized as a global address!");

        assert!(!ah2.get_acknowledgment(), "ah2 requires an acknowledgment while it does not!");
        assert!(ah3.get_acknowledgment(), "ah3 do not require acknowledgment while it does.");
    }

    #[test]
    fn frame_accessor_address_header() {
        let ah1 = AddressHeader::new(0b0011111111111111, false);
        let ah2 = AddressHeader::new(0b0011111111111111, true);      
        let ah3 = AddressHeader::new(0b0000000000000000, false);
        let ah4 = AddressHeader::new(0b0000000000000000, true);      
        let ahg1 = AddressHeader::new_global(false);
        let ahg2 = AddressHeader::new_global(true);        

        // is_global
        assert!(!ah1.is_global(), "ah1 is not a global address!");
        assert!(!ah2.is_global(), "ah2 is not a global address!");
        assert!(!ah3.is_global(), "ah3 is not a global address!");
        assert!(!ah4.is_global(), "ah4 is not a global address!");
        assert!(ahg1.is_global(), "ahg1 is a global address!");
        assert!(ahg1.is_global(), "ahg2 is a global address!");

        // get_acknowledgment
        assert!(!ah1.get_acknowledgment(), "ah1 does not require an acknowledgment!");
        assert!(!ah3.get_acknowledgment(), "ah3 does not require an acknowledgment!");
        assert!(!ahg1.get_acknowledgment(), "ahg1 does not require an acknowledgment!");
        assert!(ah2.get_acknowledgment(),   "ah2 requires an acknowledgment!");
        assert!(ah4.get_acknowledgment(),   "ah4 requires an acknowledgment!");
        assert!(ahg2.get_acknowledgment(), "ahg2 requires an acknowledgment!");

        // get_address
        assert!(ah1.get_address() == 0b0011111111111111, "ah1.get_address() returned {}, expected {}!", ah1.get_address(), 0b0011111111111111);
        assert!(ah2.get_address() == 0b0011111111111111, "ah2.get_address() returned {}, expected {}!", ah2.get_address(), 0b0011111111111111);
        assert!(ah3.get_address() == 0b0000000000000000, "ah2.get_address() returned {}, expected {}!", ah3.get_address(), 0b0000000000000000);
        assert!(ah4.get_address() == 0b0000000000000000, "ah4.get_address() returned {}, expected {}!", ah4.get_address(), 0b0000000000000000);
        assert!(ahg1.get_address() == GLOBAL_NO_ACKNOWLEDGMENT, "ahg1.get_address() returned {}, expected {}!", ahg1.get_address(), GLOBAL_NO_ACKNOWLEDGMENT);
        assert!(ahg2.get_address() == GLOBAL_NO_ACKNOWLEDGMENT, "ahg2.get_address() returned {}, expected {}!", ahg2.get_address(), GLOBAL_NO_ACKNOWLEDGMENT);
    }

    #[test]
    fn frame_modifier_address_header() {
        let mut ah1 = AddressHeader::new(0b0000000000000000, false);
        let mut ah2 = AddressHeader::new(0b0000000000000000, true);      
        let mut ahg1 = AddressHeader::new_global(false);
        let mut ahg2 = AddressHeader::new_global(true); 
        
        // set_acknowledgment 
        ah1.set_acknowledgment(true);
        assert_eq!(ah1.0, ah2.0, "ah1.set_acknowledgment(true)");
        ah1.set_acknowledgment(false);
        assert_eq!(ah1.0, 0b0000000000000000, "ah1.set_acknowledgment(false)");
        ahg1.set_acknowledgment(true);
        assert_eq!(ahg1.0, ahg2.0, "ahg1.set_acknowledgment(true)");
        ahg1.set_acknowledgment(false);
        assert_eq!(ahg1.0, GLOBAL_NO_ACKNOWLEDGMENT, "ahg1.set_acknowledgment(false)");

        // set_address
        let mut ah1 = AddressHeader::new(0b0000000000000000, false);
        ah1.set_address(GLOBAL_NO_ACKNOWLEDGMENT);
        assert_eq!(ah1.0, GLOBAL_NO_ACKNOWLEDGMENT, "ah1.set_address(GLOBAL_NO_ACKNOWLEDGMENT)");
        ah1.set_address(GLOBAL_ACKNOWLEDGMENT);
        assert_eq!(ah1.0, GLOBAL_NO_ACKNOWLEDGMENT, "ah1.set_address(GLOBAL_ACKNOWLEDGMENT) should ignore the acknowledgment!");
        ah1.set_acknowledgment(true);
        ah1.set_address(GLOBAL_NO_ACKNOWLEDGMENT);
        assert_eq!(ah1.0, GLOBAL_ACKNOWLEDGMENT, "ah1.set_address(GLOBAL_NO_ACKNOWLEDGMENT) should ignore the acknowledgment!");
        ah1.set_address(0b0000000000000000);
        assert_eq!(ah1.0, 0b1000000000000000, "ah1.set_address(0b0000000000000000) should ignore the acknowledgment!");
    }

    #[test]
    fn frame_encode_payload_flag() {
        let pf1 = PayloadFlag::new(&[1,4,6,9,15]);
        assert_eq!(pf1.0, 0b1000_0010_0101_0010);
        let pf2 = PayloadFlag::new(&[3,5,8,12,13]);
        assert_eq!(pf2.0, 0b0011_0001_0010_1000);
        let pf3 = PayloadFlag::new(&[2,7,10,11,14]);
        assert_eq!(pf3.0, 0b0100_1100_1000_0100);
        let pf4 = PayloadFlag::new(&[14,11,10,7,2]);
        assert_eq!(pf4.0, 0b0100_1100_1000_0100);
        let mut pf5 = PayloadFlag::new(&[14,11,10,7,2]);
        pf5.push(3);
        pf5.push(5);
        pf5.push(12);
        pf5.push(13);
        pf5.push(8);
        assert_eq!(pf5.0, 0b0111_1101_1010_1100);
    }

    #[test]
    fn frame_decode_payload_flag() {
        let pf1 = PayloadFlag::new(&[1,4,6,9,15]);
        let pf2 = PayloadFlag::new(&[3,5,8,12,13]);
        let pf3 = PayloadFlag::new(&[2,7,10,11,14]);
        let pf4 = PayloadFlag::new(&[14,11,10,7,2]);

        // TO CONTINUE!
    }

    #[test]
    fn frame_decode_info_header() {
        let ih1 = InfoHeader::new(0, 0);
        assert!(ih1.get_recipients() == 0, "ih1::get_recipients() returned {} recipients while {} was expected!", ih1.get_recipients(), 0);
        assert!(ih1.get_frames() == 0, "ih1::get_frames() returned {} frames while {} was expected!", ih1.get_frames(), 0);

        let ih2 = InfoHeader::new(15, 0);
        assert!(ih2.get_recipients() == 15, "ih2::get_recipients() returned {} recipients while {} was expected!", ih2.get_recipients(), 15);
        assert!(ih2.get_frames() == 0, "ih2::get_frames() returned {} frames while {} was expected!", ih2.get_frames(), 0);

        let ih3 = InfoHeader::new(0,5);
        assert!(ih3.get_recipients() == 0, "ih3::get_recipients() returned {} recipients while {} was expected!", ih3.get_recipients(), 0);
        assert!(ih3.get_frames() == 5, "ih3::get_frames() returned {} frames while {} was expected!", ih3.get_frames(), 5);

        let ih4 = InfoHeader::new(12, 3);
        assert!(ih4.get_recipients() == 12, "ih4::get_recipients() returned {} recipients while {} was expected!", ih4.get_recipients(), 12);
        assert!(ih4.get_frames() == 3, "ih4::get_frames() returned {} frames while {} was expected!", ih4.get_frames(), 3);
    }
}