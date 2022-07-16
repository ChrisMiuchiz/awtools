use aw_core::*;
use rand::Rng;

use crate::system;
use crate::{
    client::{Client, ClientID, ClientManager, Heartbeat},
    config,
    database::Database,
    packet_handler,
    universe_license::LicenseGenerator,
};
use std::net::{SocketAddrV4, TcpListener};

pub struct UniverseServer {
    config: config::UniverseConfig,
    license_generator: LicenseGenerator,
    pub client_manager: ClientManager,
    pub database: Database,
    listener: TcpListener,
    pub universe: hecs::World,
}

impl UniverseServer {
    pub fn new(config: config::Config) -> Result<Self, String> {
        let database = Database::new(config.mysql, &config.universe)?;
        let ip = SocketAddrV4::new(config.universe.ip, config.universe.port);
        let listener = TcpListener::bind(&ip).unwrap();
        listener.set_nonblocking(true).unwrap();

        Ok(Self {
            config: config.universe,
            license_generator: LicenseGenerator::new(&ip),
            client_manager: Default::default(),
            database,
            listener,
            universe: hecs::World::new(),
        })
    }

    pub fn run(&mut self) {
        log::info!(
            "Starting universe on {}:{}",
            self.config.ip,
            self.config.port
        );
        loop {
            self.accept_new_clients();
            self.service_clients();
            system::send_heartbeats(self);
            system::purge_dead_clients(self);
        }
    }

    fn accept_new_clients(&mut self) {
        while let Ok((stream, addr)) = self.listener.accept() {
            // TODO: generate IDs that can't collide
            let id = ClientID(rand::thread_rng().gen_range(0..=usize::MAX));
            let client = Client::new(AWConnection::new(AWProtocol::new(stream)), addr, id);
            self.client_manager.add_client(client);

            self.universe.spawn((id, Heartbeat { last_time: 0 }));
        }
    }

    fn service_clients(&mut self) {
        for client in self.client_manager.clients() {
            let messages = client.connection.recv();
            self.handle_messages(messages, client);
        }
    }

    fn handle_messages(&self, messages: Vec<ProtocolMessage>, client: &Client) {
        for message in messages {
            match message {
                ProtocolMessage::Packet(packet) => {
                    self.handle_packet(&packet, client);
                }
                ProtocolMessage::Disconnect => {
                    client.kill();
                }
                ProtocolMessage::StreamKey(_)
                | ProtocolMessage::Encrypt(_)
                | ProtocolMessage::PacketGroup(_) => {
                    panic!("Should not receive these message types on this end.");
                }
            }
        }
    }

    fn handle_packet(&self, packet: &AWPacket, client: &Client) {
        log::debug!("Handling packet {packet:?}");
        match packet.get_opcode() {
            PacketType::PublicKeyRequest => packet_handler::public_key_request(client),
            PacketType::StreamKeyResponse => {
                packet_handler::stream_key_response(client, packet, &self.database)
            }
            PacketType::PublicKeyResponse => packet_handler::public_key_response(client, packet),
            PacketType::Login => packet_handler::login(
                client,
                packet,
                &self.client_manager,
                &self.license_generator,
                &self.database,
            ),
            PacketType::Heartbeat => packet_handler::heartbeat(client),
            PacketType::WorldServerStart => packet_handler::world_server_start(client, packet),
            PacketType::UserList => packet_handler::user_list(client, packet, &self.client_manager),
            PacketType::AttributeChange => packet_handler::attribute_change(
                client,
                packet,
                &self.database,
                &self.client_manager,
            ),
            PacketType::CitizenNext => packet_handler::citizen_next(client, packet, &self.database),
            PacketType::CitizenPrev => packet_handler::citizen_prev(client, packet, &self.database),
            PacketType::CitizenLookupByName => {
                packet_handler::citizen_lookup_by_name(client, packet, &self.database)
            }
            PacketType::CitizenLookupByNumber => {
                packet_handler::citizen_lookup_by_number(client, packet, &self.database)
            }
            PacketType::CitizenChange => {
                packet_handler::citizen_change(client, packet, &self.database)
            }
            PacketType::LicenseAdd => packet_handler::license_add(client, packet, &self.database),
            PacketType::LicenseByName => {
                packet_handler::license_by_name(client, packet, &self.database)
            }
            PacketType::LicenseNext => packet_handler::license_next(client, packet, &self.database),
            PacketType::LicensePrev => packet_handler::license_prev(client, packet, &self.database),
            PacketType::LicenseChange => {
                packet_handler::license_change(client, packet, &self.database)
            }
            PacketType::WorldStart => {
                packet_handler::world_start(client, packet, &self.database, &self.client_manager)
            }
            PacketType::WorldStop => {
                packet_handler::world_stop(client, packet, &self.client_manager)
            }
            PacketType::WorldList => {
                packet_handler::world_list(client, packet, &self.client_manager)
            }
            PacketType::WorldLookup => {
                packet_handler::world_lookup(client, packet, &self.client_manager)
            }
            PacketType::Identify => {
                packet_handler::identify(client, packet, &self.client_manager, &self.database)
            }
            PacketType::WorldStatsUpdate => {
                packet_handler::world_stats_update(client, packet, &self.client_manager)
            }
            PacketType::CitizenAdd => packet_handler::citizen_add(client, packet, &self.database),
            PacketType::ContactAdd => {
                packet_handler::contact_add(client, packet, &self.database, &self.client_manager)
            }
            PacketType::TelegramSend => {
                packet_handler::telegram_send(client, packet, &self.database, &self.client_manager)
            }
            PacketType::TelegramGet => {
                packet_handler::telegram_get(client, packet, &self.database);
            }
            PacketType::SetAFK => packet_handler::set_afk(client, packet),
            PacketType::ContactConfirm => packet_handler::contact_confirm(
                client,
                packet,
                &self.database,
                &self.client_manager,
            ),
            PacketType::ContactList => {
                packet_handler::contact_list(client, packet, &self.database, &self.client_manager)
            }
            _ => {
                log::info!("Unhandled packet {packet:?}");
            }
        }
    }
}
