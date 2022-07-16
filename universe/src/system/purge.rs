use crate::{
    client::{ClientID, Entity},
    packet_handler::{self, update_contacts_of_user},
    player::{PlayerInfo, PlayerState},
    world::World,
    UniverseServer,
};

pub fn purge_dead_clients(server: &mut UniverseServer) {
    let mut remove_entities = Vec::<hecs::Entity>::new();

    for (e, id) in server.universe.query::<&ClientID>().iter() {
        let client = server
            .client_manager
            .get(*id)
            .expect("Every ClientID should have a client.");

        if client.is_dead() {
            log::info!("Disconnected {}", client.addr.ip());
            if let Some(Entity::WorldServer(server_info)) = &mut client.info_mut().entity {
                packet_handler::world_server_hide_all(server_info);
            }
            if let Some(Entity::WorldServer(server_info)) = &client.info().entity {
                World::send_updates_to_all(&server_info.worlds, &server.client_manager);
            }

            if let Some(Entity::Player(player)) = &mut client.info_mut().entity {
                player.state = PlayerState::Offline;
            }
            if let Some(Entity::Player(player)) = &client.info().entity {
                PlayerInfo::send_update_to_all(player, &server.client_manager);

                if let Some(citizen_id) = player.citizen_id {
                    // Update the user's friends to tell them this user is now offline
                    update_contacts_of_user(citizen_id, &server.database, &server.client_manager);
                }
            }
            server.client_manager.remove_client(*id);
            remove_entities.push(e);
        }
    }

    for e in remove_entities {
        server
            .universe
            .despawn(e)
            .expect("Only existent entities should be removed.");
    }
}
