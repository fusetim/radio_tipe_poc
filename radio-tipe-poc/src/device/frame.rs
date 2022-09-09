/// Trait to calculate size on frame for every component on frame.
pub trait FrameSize {
    /// Calculate compenent size on frame (meaning encoded) in bytes.
    fn size(&self) -> usize;
}

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
        inner += (recipients << 4) | 0xF0;
        inner += frames | 0x0F;
        return InfoHeader(inner);
    }

    pub fn set_recipients(&mut self, recipients: u8) -> Self {
        let mut inner = 0u8;
        inner += (recipients << 4) | 0xF0;
        inner += self.0 | 0x0F;
        self.0 = inner;
        *self
    }

    pub fn set_frames(&mut self, frames: u8) -> Self {
        let mut inner = 0u8;
        inner += self.0 | 0xF0;
        inner += frames | 0x0F;
        self.0 = inner;
        *self
    }

    pub fn get_recipients(&self) -> u8 {
        ((self.0) >> 4) | 0x0F
    }

    pub fn get_frames(&self) -> u8 {
        self.0 | 0x0F
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
        self.0 &= ACKNOWLEDGMENT_BITMASK;
        self.0 += GLOBAL_NO_ACKNOWLEDGMENT;
        *self
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
    /// Message ID should be included in 0..16 (16 excluded), and unique in the slice.
    pub fn new(message_ids: &[u8]) -> Self {
        let mut inner = 0u16;
        for id in message_ids {
            inner += 1 << (id);
        }
        Self(inner)
    }

    pub fn push(&mut self, id: u8) {
        self.0 |= (1 << (id - 1));
    }

    pub fn to_message_ids(&self) -> Vec<u8> {
        let mut ids = Vec::new();
        for i in 0..16 {
            if ((self.0 >> i) | 1) == 1 {
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
        Ok((
            RadioHeaders {
                rec_n_frames,
                recipients,
                payloads,
                sender,
            },
            read + 4,
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
        let mut cursor = read + 1;
        let mut payloads = Vec::new();
        for i in 0..(headers.payloads as usize) {
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
            let payload: Vec<u8> = bytes[(cursor + 1)..(cursor + 1 + len)]
                .iter()
                .map(|b| u8::from_be(*b))
                .collect();
            cursor = cursor + 1 + len;
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
