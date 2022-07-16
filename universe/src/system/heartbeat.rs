use std::time::{SystemTime, UNIX_EPOCH};

use aw_core::{AWPacket, PacketType};

use crate::{
    client::{ClientID, Heartbeat},
    UniverseServer,
};

pub fn send_heartbeats(server: &mut UniverseServer) {
    for (e, (heartbeat, id)) in server.universe.query_mut::<(&mut Heartbeat, &ClientID)>() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Current time is before the unix epoch.")
            .as_secs();

        let client = server
            .client_manager
            .get(*id)
            .expect("Every ClientID should have a client.");

        // 30 seconds between each heartbeat
        let next_heartbeat = heartbeat.last_time + 30;

        if next_heartbeat <= now {
            log::info!("Sending heartbeat to {}", client.addr.ip());
            let packet = AWPacket::new(PacketType::Heartbeat);
            client.connection.send(packet);
            heartbeat.last_time = now;
        }
    }
}
