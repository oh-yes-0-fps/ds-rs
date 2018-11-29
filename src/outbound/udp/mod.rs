pub mod types;

use byteorder::{WriteBytesExt, BigEndian};
use self::types::*;
use self::types::tags::*;

pub struct UdpControlPacket {
    pub seqnum: u16,
    pub control: Control,
    pub request: Option<Box<Request>>,
    pub alliance: Alliance,
    pub tags: Vec<Box<Tag>>,
}

impl UdpControlPacket {

    pub fn new() -> UdpControlPacket {
        UdpControlPacket {
            seqnum: 1,
            control: Control::empty(),
            request: None,
            alliance: Alliance::new_red(1),
            tags: Vec::new()
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = vec![];
        buf.write_u16::<BigEndian>(self.seqnum).unwrap();
        buf.push(0x01); // comm version
        buf.push(self.control.bits());
        match &self.request {
            Some(ref req) => buf.push(req.code()),
            None => buf.push(0)
        }

        buf.push(self.alliance.0);


        for tag in &self.tags {
            buf.extend(tag.construct());
        }

        buf
    }

    pub fn increment_seqnum(&mut self) {
        self.seqnum += 1;
    }
}