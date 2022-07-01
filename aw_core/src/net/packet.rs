//! Packet (de)serialization for AW
use crate::net::packet_var::{AWPacketVar, VarID};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use std::io::Cursor;

#[derive(Debug, PartialEq)]
struct AWPacket {
    vars: Vec<AWPacketVar>,
    opcode: PacketType,
    header_0: u16,
    header_1: u16,
}

impl AWPacket {
    pub fn new(opcode: PacketType) -> Self {
        Self {
            vars: Vec::new(),
            opcode,
            header_0: 0,
            header_1: 0,
        }
    }

    pub fn add_var(&mut self, var: AWPacketVar) {
        self.vars.push(var);
    }

    pub fn get_var(&self, var_id: VarID) -> Option<&AWPacketVar> {
        for var in &self.vars {
            if var.get_var_id() == var_id {
                return Some(var);
            }
        }
        None
    }

    pub fn get_byte(&self, var_id: VarID) -> Option<u8> {
        for var in &self.vars {
            match var {
                AWPacketVar::Byte(id, x) if *id == var_id => return Some(*x),
                _ => {}
            }
        }

        None
    }

    pub fn get_int(&self, var_id: VarID) -> Option<i32> {
        for var in &self.vars {
            match var {
                AWPacketVar::Int(id, x) if *id == var_id => return Some(*x),
                _ => {}
            }
        }

        None
    }

    pub fn get_float(&self, var_id: VarID) -> Option<f32> {
        for var in &self.vars {
            match var {
                AWPacketVar::Float(id, x) if *id == var_id => return Some(*x),
                _ => {}
            }
        }

        None
    }

    pub fn get_string(&self, var_id: VarID) -> Option<String> {
        for var in &self.vars {
            match var {
                AWPacketVar::String(id, x) if *id == var_id => return Some(x.clone()),
                _ => {}
            }
        }

        None
    }

    pub fn get_data(&self, var_id: VarID) -> Option<Vec<u8>> {
        for var in &self.vars {
            match var {
                AWPacketVar::Data(id, x) if *id == var_id => return Some(x.clone()),
                _ => {}
            }
        }

        None
    }

    fn serialize_len(&self) -> usize {
        let mut size = TagHeader::length();

        for var in &self.vars {
            size += var.serialize_len();
        }

        size
    }

    pub fn serialize(&self) -> Result<Vec<u8>, String> {
        let serialize_len = self.serialize_len();

        if serialize_len > u16::MAX.into() {
            return Err(format!("Serializing packet too large: {serialize_len}"));
        }

        let mut result = Vec::<u8>::with_capacity(serialize_len);
        let serialize_len = serialize_len as u16;

        let header = TagHeader {
            serialized_length: serialize_len,
            header_0: self.header_0,
            opcode: self.opcode as i16,
            header_1: self.header_1,
            var_count: self.vars.len() as u16,
        };

        result.extend(header.serialize());
        for var in &self.vars {
            result.extend(var.serialize()?);
        }

        Ok(result)
    }

    pub fn deserialize(mut data: &[u8]) -> Result<(Self, usize), String> {
        let mut total_consumed: usize = 0;
        let (header, consumed) = TagHeader::deserialize(data)?;
        data = &data[consumed..];
        total_consumed += consumed;

        let mut vars = Vec::<AWPacketVar>::with_capacity(header.var_count as usize);

        for _ in 0..header.var_count {
            let (var, consumed) = AWPacketVar::deserialize(data)?;
            data = &data[consumed..];
            total_consumed += consumed;

            vars.push(var);
        }

        if total_consumed != header.serialized_length.into() {
            return Err(format!(
                "Consumed {total_consumed} bytes instead of {}",
                header.serialized_length
            ));
        }

        let opcode = PacketType::from_i16(header.opcode).unwrap_or_else(|| {
            eprintln!("Deserialized unknown packet ID {}", header.opcode);
            PacketType::Unknown
        });

        Ok((
            Self {
                vars,
                opcode,
                header_0: header.header_0,
                header_1: header.header_1,
            },
            total_consumed,
        ))
    }
}

struct TagHeader {
    /// The length of the packet
    pub serialized_length: u16,
    /// Purpose not known
    pub header_0: u16,
    /// Packet type
    pub opcode: i16,
    /// Purpose not known
    pub header_1: u16,
    /// Number of variables in this packet
    pub var_count: u16,
}

impl TagHeader {
    #[inline]
    pub fn length() -> usize {
        10
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::<u8>::with_capacity(10);
        result
            .write_u16::<BigEndian>(self.serialized_length)
            .unwrap();
        result.write_u16::<BigEndian>(self.header_0).unwrap();
        result.write_i16::<BigEndian>(self.opcode).unwrap();
        result.write_u16::<BigEndian>(self.header_1).unwrap();
        result.write_u16::<BigEndian>(self.var_count).unwrap();

        // This is important because it is going over the network
        assert!(result.len() == TagHeader::length());

        result
    }

    pub fn deserialize(data: &[u8]) -> Result<(Self, usize), String> {
        assert!(data.len() >= TagHeader::length());

        let mut reader = Cursor::new(data);

        let serialized_length = reader
            .read_u16::<BigEndian>()
            .map_err(|_| "Could not read serialized_length")?;
        let header_0 = reader
            .read_u16::<BigEndian>()
            .map_err(|_| "Could not read header_0")?;
        let opcode = reader
            .read_i16::<BigEndian>()
            .map_err(|_| "Could not read opcode")?;
        let header_1 = reader
            .read_u16::<BigEndian>()
            .map_err(|_| "Could not read header_1")?;
        let var_count = reader
            .read_u16::<BigEndian>()
            .map_err(|_| "Could not read var_count")?;

        Ok((
            Self {
                serialized_length,
                header_0,
                opcode,
                header_1,
                var_count,
            },
            reader.position().try_into().unwrap(),
        ))
    }
}

#[derive(FromPrimitive, Clone, Copy, Debug, PartialEq)]
enum PacketType {
    PublicKeyResponse = 1,
    StreamKeyResponse = 2,

    Address = 5,
    Attributes = 6,
    AttributeChange = 7,
    AttributesReset = 8,
    AvatarAdd = 9,
    AvatarChange = 10,
    AvatarClick = 11,
    AvatarDelete = 12,

    Invite = 14,
    BotgramResponse = 15,

    Capabilities = 16,
    CellBegin = 17,
    CellEnd = 18,
    CellNext = 19,
    CellUpdate = 20,
    CitizenAdd = 21,
    CitizenInfo = 22,
    CitizenLookupByName = 23,
    CitizenLookupByNumber = 24,
    CitizenChange = 25,
    CitizenDelete = 26,
    CitizenNext = 27,
    CitizenPrev = 28,
    CitizenChangeResult = 29,
    ConsoleMessage = 30,
    ContactAdd = 31,
    ContactChange = 32,
    ContactDelete = 33,
    ContactList = 34,
    Enter = 35,

    PublicKeyRequest = 36,
    Heartbeat = 37,
    Identify = 38,
    LicenseAdd = 39,
    LicenseResult = 40,
    LicenseByName = 41,
    LicenseChange = 42,
    LicenseDelete = 43,
    LicenseNext = 44,
    LicensePrev = 45,
    LicenseChangeResult = 46,
    Login = 47,
    Message = 48,
    ObjectAdd = 49,

    ObjectClick = 51,
    ObjectDelete = 52,
    ObjectDeleteAll = 53,

    ObjectResult = 55,
    ObjectSelect = 56,

    QueryNeedMore = 59,
    QueryUpToDate = 60,
    RegistryReload = 61,
    ServerLogin = 62,
    WorldServerStart = 63,

    ServerWorldDelete = 67,
    ServerWorldList = 68,
    ServerWorldListResult = 69,
    ServerWorldResult = 70,

    TelegramDeliver = 75,
    TelegramGet = 76,
    TelegramNotify = 77,
    TelegramSend = 78,
    Teleport = 79,
    TerrainBegin = 80,
    TerrainChanged = 81,
    TerrainData = 82,
    TerrainDelete = 83,
    TerrainEnd = 84,
    TerrainLoad = 85,
    TerrainNext = 86,

    TerrainSet = 88,
    ToolbarClick = 89,
    URL = 90,
    URLClick = 91,
    UserList = 92,
    UserListResult = 93,
    LoginApplication = 94,

    WorldList = 96,
    WorldListResult = 97,
    WorldLookup = 98,
    WorldStart = 99,
    WorldStop = 100,
    Tunnel = 101,
    WorldStatsUpdate = 102,
    Join = 103,
    JoinReply = 104,
    Xfer = 105,
    XferReply = 106,
    Noise = 107,

    Camera = 109,
    Botmenu = 110,
    BotmenuResult = 111,
    WorldEject = 112,
    EjectAdd = 113,
    EjectDelete = 114,
    EjectLookup = 115,
    EjectNext = 116,
    EjectPrev = 117,
    WorldEjectResult = 118,
    WorldConnectionResult = 119,
    ObjectBump = 120,
    PasswordSend = 121,

    CavTemplateByNumber = 123,
    CavTemplateNext = 124,
    CavTemplateChange = 125,
    CavTemplateDelete = 126,
    WorldCAVDefinitionChange = 127,
    WorldCAV = 128,

    CavDelete = 130,
    WorldCAVResult = 131,
    MoverAdd = 144,
    MoverDelete = 145,
    MoverChange = 146,

    MoverRiderAdd = 148,
    MoverRiderDelete = 149,
    MoverRiderChange = 150,
    MoverLinks = 151,

    SetAFK = 152,

    Immigrate = 155,

    Register = 157,

    AvatarReload = 159,
    WorldInstanceSet = 160,
    WorldInstanceGet = 161,

    ContactConfirm = 163,

    HudCreate = 164,
    HudClick = 165,
    HudDestroy = 166,
    HudClear = 167,
    HudResult = 168,
    AvatarLocation = 169,
    ObjectQuery = 170,
    LaserBeam = 183,

    Unknown = 0x7FFF,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_serialize() {
        let mut packet = AWPacket::new(PacketType::Address);
        packet.add_var(AWPacketVar::String(VarID::AFKStatus, "Hello".to_string()));
        packet.add_var(AWPacketVar::Byte(VarID::Attrib_AllowTourists, 1));
        let serialized = packet.serialize().unwrap();
        let (deserialized, _) = AWPacket::deserialize(&serialized).unwrap();
        assert!(packet == deserialized);
    }
}
