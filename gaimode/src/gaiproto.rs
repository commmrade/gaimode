#[derive(Debug)]
pub struct Gaiproto {
    size: u32,
    kind: u16,
    payload: Vec<u8>,
}

pub const MIN_PACKET_SIZE: usize = 6;

pub const K_OPTIMIZE_PROCESS: u16 = 0x2;
pub const K_RESET_PROCESS: u16 = 0x4;
pub const K_RESET_ALL: u16 = 0x6;

impl Gaiproto {
    pub fn new(size: u32, kind: u16, payload: Vec<u8>) -> Gaiproto {
        Gaiproto {
            size,
            kind,
            payload,
        }
    }
    pub fn convert_to_bytes(self) -> Vec<u8> {
        self.into()
    }
    pub fn from_bytes(bytes: Vec<u8>) -> Gaiproto {
        bytes.into()
    }
}

impl Into<Vec<u8>> for Gaiproto {
    fn into(self) -> Vec<u8> {
        let mut res = Vec::new();
        res.reserve(std::mem::size_of::<u32>() + std::mem::size_of::<u16>() + self.payload.len());
        res.extend_from_slice(&self.size.to_be_bytes());
        res.extend_from_slice(&self.kind.to_be_bytes());
        res.extend_from_slice(&self.payload);
        res
    }
}

impl From<Vec<u8>> for Gaiproto {
    fn from(value: Vec<u8>) -> Self {
        let cursor = &value[0..];

        let size = u32::from_be_bytes(cursor[0..std::mem::size_of::<u32>()].try_into().unwrap());
        let cursor = &cursor[std::mem::size_of_val(&size)..];

        let kind = u16::from_be_bytes(cursor[0..std::mem::size_of::<u16>()].try_into().unwrap());
        let cursor = &cursor[std::mem::size_of_val(&kind)..];

        let payload_size: usize =
            size as usize - (std::mem::size_of_val(&size) + std::mem::size_of_val(&kind));
        let payload = cursor[0..payload_size].to_owned();

        Self {
            size,
            kind,
            payload,
        }
    }
}
