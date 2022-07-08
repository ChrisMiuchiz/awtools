use std::{
    net::IpAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    attributes,
    attributes::set_attribute,
    client::{Client, ClientManager, ClientType, Entity, PlayerInfo},
    database::citizen::CitizenQuery,
    database::Database,
    database::CitizenDB,
    license::LicenseGenerator,
};
use aw_core::*;
use num_traits::FromPrimitive;

/// Represents the credentials obtained during handling of the Login packet.
struct LoginCredentials {
    pub user_type: Option<ClientType>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub email: Option<String>,
    pub privilege_id: Option<u32>,
    pub privilege_password: Option<String>,
}

impl LoginCredentials {
    /// Parses login credentials from a packet.
    pub fn from_packet(packet: &AWPacket) -> Self {
        Self {
            user_type: packet
                .get_int(VarID::UserType)
                .map(ClientType::from_i32)
                .unwrap(),
            username: packet.get_string(VarID::LoginUsername),
            password: packet.get_string(VarID::Password),
            email: packet.get_string(VarID::Email),
            privilege_id: packet.get_uint(VarID::PrivilegeUserID),
            privilege_password: packet.get_string(VarID::PrivilegePassword),
        }
    }
}

/// Handle a client attempting to log in.
pub fn login(
    client: &Client,
    packet: &AWPacket,
    client_manager: &ClientManager,
    license_generator: &LicenseGenerator,
    database: &Database,
) {
    let _client_version = packet.get_int(VarID::BrowserVersion);
    let browser_build = packet.get_int(VarID::BrowserBuild);

    let credentials = LoginCredentials::from_packet(packet);

    let mut response = AWPacket::new(PacketType::Login);

    let rc = match validate_login(client, &credentials, client_manager, database) {
        // Successful login
        Ok(user) => {
            match (user, credentials.user_type) {
                // Promote to citizen
                (Some(citizen), Some(ClientType::UnspecifiedHuman)) => {
                    client.info_mut().client_type = Some(ClientType::Citizen);

                    let client_entity = Entity::Player(PlayerInfo {
                        build: browser_build.unwrap_or(0),
                        session_id: client_manager.create_session_id(),
                        citizen_id: Some(citizen.id),
                        privilege_id: credentials.privilege_id,
                        username: citizen.name,
                    });

                    client.info_mut().entity = Some(client_entity);

                    // Add packet variables with citizen info
                    response.add_var(AWPacketVar::Uint(VarID::BetaUser, citizen.beta));
                    response.add_var(AWPacketVar::Uint(VarID::TrialUser, citizen.trial));
                    response.add_var(AWPacketVar::Uint(VarID::CitizenNumber, citizen.id));
                    response.add_var(AWPacketVar::Uint(VarID::CitizenPrivacy, citizen.privacy));
                    response.add_var(AWPacketVar::Uint(VarID::CAVEnabled, citizen.cav_enabled));

                    // TODO: update login time and last address
                }
                // Promote to tourist
                (None, Some(ClientType::UnspecifiedHuman)) => {
                    client.info_mut().client_type = Some(ClientType::Tourist);

                    let client_entity = Entity::Player(PlayerInfo {
                        build: browser_build.unwrap_or(0),
                        session_id: client_manager.create_session_id(),
                        citizen_id: None,
                        privilege_id: None,
                        username: credentials.username.unwrap_or_default(),
                    });

                    client.info_mut().entity = Some(client_entity);
                }
                (_, Some(ClientType::Bot)) => {
                    todo!();
                }
                _ => {
                    panic!("Got an OK login validation that wasn't a citizen, tourist, or bot. Should be impossible.");
                }
            }
            ReasonCode::Success
        }
        // Failed, either because of incorrect credentials or because the client is of the wrong type
        Err(reason) => reason,
    };

    // Inform the client of their displayed username and their new session ID
    if let Some(Entity::Player(info)) = &client.info_mut().entity {
        response.add_var(AWPacketVar::String(
            VarID::CitizenName,
            info.username.clone(),
        ));
        response.add_var(AWPacketVar::Int(VarID::SessionID, info.session_id as i32));
    }

    // Add license data (Specific to the IP/port binding that the client sees!)
    response.add_var(AWPacketVar::Data(
        VarID::UniverseLicense,
        license_generator.create_license_data(browser_build.unwrap_or(0)),
    ));

    response.add_var(AWPacketVar::Int(VarID::ReasonCode, rc as i32));
    client.connection.send(response);
}

/// Validates a client's login credentials.
/// This includes ensuring a valid username, the correct password(s) if applicable,
/// and the correct user type (world/bot/citizen/tourist).
/// Returns information about the citizen whose credentials matched (if not a tourist),
/// or returns a ReasonCode if login should fail.
fn validate_login(
    client: &Client,
    credentials: &LoginCredentials,
    client_manager: &ClientManager,
    database: &Database,
) -> Result<Option<CitizenQuery>, ReasonCode> {
    match credentials.user_type {
        Some(ClientType::Bot) => todo!(),
        Some(ClientType::UnspecifiedHuman) => {
            validate_human_login(client, credentials, client_manager, database)
        }
        _ => Err(ReasonCode::NoSuchCitizen),
    }
}

/// Validate's human's login credentials. This applies to tourists and citizens
/// but not bots or worlds.
/// Returns information about the citizen whose credentials matched (if not a tourist),
/// or returns a ReasonCode if login should fail.
fn validate_human_login(
    client: &Client,
    credentials: &LoginCredentials,
    client_manager: &ClientManager,
    database: &Database,
) -> Result<Option<CitizenQuery>, ReasonCode> {
    let username = credentials
        .username
        .as_ref()
        .ok_or(ReasonCode::NoSuchCitizen)?;

    // A user is a tourist if they have quotes around their name
    if username.starts_with('"') {
        client_manager.check_tourist(username)?;
        Ok(None)
    } else {
        let cit = client_manager.check_citizen(
            database,
            client,
            &credentials.username,
            &credentials.password,
            credentials.privilege_id,
            &credentials.privilege_password,
        )?;
        Ok(Some(cit))
    }
}

pub fn heartbeat(client: &Client) {
    log::info!("Received heartbeat from {}", client.addr.ip());
}

fn ip_to_num(ip: IpAddr) -> u32 {
    let mut res: u32 = 0;
    if let std::net::IpAddr::V4(v4) = ip {
        for octet in v4.octets().iter().rev() {
            res <<= 8;
            res |= *octet as u32;
        }
    }
    res
}

pub fn user_list(client: &Client, packet: &AWPacket, client_manager: &ClientManager) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Current time is before the unix epoch.")
        .as_secs() as i32;

    // I am not entirely sure what the purpose of this is, but it has some sort
    // of relation to 3 days. It sends our values back to us with this, so we
    // can use this to deny the client from spamming for updates, which causes
    // flickering of the user list with very large numbers of players.
    let time_val = packet.get_int(VarID::UserList3DayUnknown).unwrap_or(0);
    if now.saturating_sub(3) < time_val {
        return;
    }

    // Group packets into larger transmissions for efficiency
    let mut group = AWPacketGroup::new();

    for client in client_manager.clients() {
        if let Some(Entity::Player(info)) = &client.info().entity {
            // Make a new UserList packet for each user in this list
            let mut p = AWPacket::new(PacketType::UserList);

            // Client also expects var 178 as a string, but don't know what it is for.
            // p.add_var(AWPacketVar::String(VarID::UserList178, format!("178")));
            p.add_var(AWPacketVar::String(
                VarID::UserListName,
                info.username.clone(),
            ));

            // ID is supposed to be an ID relating to the user list so it can
            // be updated when a user changes state, but using the session id
            // for this is convenient for now.
            p.add_var(AWPacketVar::Int(VarID::UserListID, info.session_id.into()));

            p.add_var(AWPacketVar::Uint(
                VarID::UserListCitizenID,
                info.citizen_id.unwrap_or(0),
            ));
            p.add_var(AWPacketVar::Uint(
                VarID::UserListPrivilegeID,
                info.privilege_id.unwrap_or(0),
            ));
            if client.has_admin_permissions() {
                p.add_var(AWPacketVar::Uint(
                    VarID::UserListAddress,
                    ip_to_num(client.addr.ip()),
                ));
            }
            p.add_var(AWPacketVar::Byte(VarID::UserListState, 1)); // TODO: this means online
            p.add_var(AWPacketVar::String(
                VarID::UserListWorldName,
                "NO WORLD".to_string(),
            )); // TODO: No worlds yet

            if let Err(p) = group.push(p) {
                // If the current group is full, send it and start a new one
                client.connection.send_group(group);
                group = AWPacketGroup::new();

                let mut more = AWPacket::new(PacketType::UserListResult);
                // Yes, expect another UserList packet from the server
                more.add_var(AWPacketVar::Byte(VarID::UserListMore, 1));
                more.add_var(AWPacketVar::Int(VarID::UserList3DayUnknown, now));
                group.push(more).ok();
                group.push(p).ok();
            }
        }
    }

    // Send packet indicating that the server is done
    let mut p = AWPacket::new(PacketType::UserListResult);
    p.add_var(AWPacketVar::Byte(VarID::UserListMore, 0));
    p.add_var(AWPacketVar::Int(VarID::UserList3DayUnknown, now));

    if let Err(p) = group.push(p) {
        client.connection.send_group(group);
        group = AWPacketGroup::new();
        group.push(p).ok();
    }

    client.connection.send_group(group);
}

pub fn attribute_change(
    client: &Client,
    packet: &AWPacket,
    database: &Database,
    client_manager: &ClientManager,
) {
    // Only admins should be able to change Universe attributes
    if !client.has_admin_permissions() {
        log::info!(
            "Client {} tried to set attributes but is not an admin",
            client.addr.ip()
        );
        return;
    }

    for var in packet.get_vars().iter() {
        if let AWPacketVar::String(id, val) = var {
            log::info!("Client {} setting {:?} to {:?}", client.addr.ip(), id, val);
            set_attribute(*id, &val, database).ok();
        }
    }

    for client in client_manager.clients() {
        attributes::send_attributes(client, database);
    }
}

pub fn citizen_next(client: &Client, packet: &AWPacket, database: &Database) {
    let mut rc = ReasonCode::Success;
    let mut response = AWPacket::new(PacketType::CitizenInfo);

    if !client.has_admin_permissions() {
        log::info!(
            "Client {} tried to use CitizenNext but is not an admin",
            client.addr.ip()
        );
        rc = ReasonCode::Unauthorized;
    } else if let Some(Entity::Player(info)) = &client.info().entity {
        let citizen_id = packet.get_uint(VarID::CitizenNumber).unwrap_or(0);
        match database.citizen_by_number(citizen_id.saturating_add(1)) {
            Ok(citizen) => {
                let same_citizen_id = Some(citizen.id) == info.citizen_id;
                let is_admin = client.has_admin_permissions();
                let vars = citizen_info_vars(&citizen, same_citizen_id, is_admin);
                for v in vars {
                    response.add_var(v);
                }
            }
            Err(_) => {
                rc = ReasonCode::NoSuchCitizen;
            }
        }
    }

    response.add_var(AWPacketVar::Int(VarID::ReasonCode, rc as i32));

    client.connection.send(response);
}

pub fn citizen_prev(client: &Client, packet: &AWPacket, database: &Database) {
    let mut rc = ReasonCode::Success;
    let mut response = AWPacket::new(PacketType::CitizenInfo);

    if !client.has_admin_permissions() {
        log::info!(
            "Client {} tried to use CitizenPrev but is not an admin",
            client.addr.ip()
        );
        rc = ReasonCode::Unauthorized;
    } else if let Some(Entity::Player(info)) = &client.info().entity {
        let citizen_id = packet.get_uint(VarID::CitizenNumber).unwrap_or(0);
        match database.citizen_by_number(citizen_id.saturating_sub(1)) {
            Ok(citizen) => {
                let same_citizen_id = Some(citizen.id) == info.citizen_id;
                let is_admin = client.has_admin_permissions();
                let vars = citizen_info_vars(&citizen, same_citizen_id, is_admin);
                for v in vars {
                    response.add_var(v);
                }
            }
            Err(_) => {
                rc = ReasonCode::NoSuchCitizen;
            }
        }
    }

    response.add_var(AWPacketVar::Int(VarID::ReasonCode, rc as i32));

    client.connection.send(response);
}

pub fn citizen_lookup_by_name(client: &Client, packet: &AWPacket, database: &Database) {
    let mut rc = ReasonCode::Success;
    let mut response = AWPacket::new(PacketType::CitizenInfo);

    if !client.has_admin_permissions() {
        log::info!(
            "Client {} tried to use CitizenLookupByName but is not an admin",
            client.addr.ip()
        );
        rc = ReasonCode::Unauthorized;
    } else if let Some(Entity::Player(info)) = &client.info().entity {
        match packet.get_string(VarID::CitizenName) {
            Some(citizen_name) => match database.citizen_by_name(&citizen_name) {
                Ok(citizen) => {
                    let same_citizen_id = Some(citizen.id) == info.citizen_id;
                    let is_admin = client.has_admin_permissions();
                    let vars = citizen_info_vars(&citizen, same_citizen_id, is_admin);
                    for v in vars {
                        response.add_var(v);
                    }
                }
                Err(_) => {
                    rc = ReasonCode::NoSuchCitizen;
                }
            },
            None => {
                rc = ReasonCode::NoSuchCitizen;
            }
        }
    }

    response.add_var(AWPacketVar::Int(VarID::ReasonCode, rc as i32));

    client.connection.send(response);
}

pub fn citizen_lookup_by_number(client: &Client, packet: &AWPacket, database: &Database) {
    let mut rc = ReasonCode::Success;
    let mut response = AWPacket::new(PacketType::CitizenInfo);

    if !client.has_admin_permissions() {
        log::info!(
            "Client {} tried to use CitizenLookupByNumber but is not an admin",
            client.addr.ip()
        );
        rc = ReasonCode::Unauthorized;
    } else if let Some(Entity::Player(info)) = &client.info().entity {
        match packet.get_uint(VarID::CitizenNumber) {
            Some(citizen_id) => match database.citizen_by_number(citizen_id) {
                Ok(citizen) => {
                    let same_citizen_id = Some(citizen.id) == info.citizen_id;
                    let is_admin = client.has_admin_permissions();
                    let vars = citizen_info_vars(&citizen, same_citizen_id, is_admin);
                    for v in vars {
                        response.add_var(v);
                    }
                }
                Err(_) => {
                    rc = ReasonCode::NoSuchCitizen;
                }
            },
            None => {
                rc = ReasonCode::NoSuchCitizen;
            }
        }
    }

    response.add_var(AWPacketVar::Int(VarID::ReasonCode, rc as i32));

    client.connection.send(response);
}

pub fn citizen_change(client: &Client, packet: &AWPacket, database: &Database) {
    let changed_info = citizen_from_packet(packet);
    if changed_info.is_err() {
        log::trace!("Could not change citizen: {:?}", changed_info);
        return;
    }
    let changed_info = changed_info.unwrap();
    let mut rc = ReasonCode::Success;

    if let Some(Entity::Player(info)) = &client.info().entity {
        // Client needs to be the user in question or an admin
        if Some(changed_info.id) != info.citizen_id && !client.has_admin_permissions() {
            rc = ReasonCode::Unauthorized;
        } else {
            match database.citizen_by_number(changed_info.id) {
                Ok(original_info) => {
                    if let Err(x) = modify_citizen(
                        &original_info,
                        &changed_info,
                        database,
                        client.has_admin_permissions(),
                    ) {
                        rc = x;
                    }
                }
                Err(_) => {
                    rc = ReasonCode::NoSuchCitizen;
                }
            }
        }
    }

    let mut response = AWPacket::new(PacketType::CitizenChangeResult);
    log::trace!("Change citizen: {:?}", rc);
    response.add_var(AWPacketVar::Int(VarID::ReasonCode, rc as i32));

    client.connection.send(response);
}

fn modify_citizen(
    original: &CitizenQuery,
    changed: &CitizenQuery,
    database: &Database,
    admin: bool,
) -> Result<(), ReasonCode> {
    // Find any citizens with the same name as the new name
    if let Ok(matching_cit) = database.citizen_by_name(&changed.name) {
        // If someone already has the name, it needs to be the same user
        if matching_cit.id != original.id {
            return Err(ReasonCode::NameAlreadyUsed);
        }
    }

    let cit_query = CitizenQuery {
        id: original.id,
        changed: 0,
        name: changed.name.clone(),
        password: changed.password.clone(),
        email: changed.email.clone(),
        priv_pass: changed.priv_pass.clone(),
        comment: if admin {
            changed.comment.clone()
        } else {
            original.comment.clone()
        },
        url: changed.url.clone(),
        immigration: original.immigration,
        expiration: if admin {
            changed.expiration
        } else {
            original.expiration
        },
        last_login: original.last_login,
        last_address: original.last_address,
        total_time: original.total_time,
        bot_limit: if admin {
            changed.bot_limit
        } else {
            original.bot_limit
        },
        beta: if admin { changed.beta } else { original.beta },
        cav_enabled: if admin {
            changed.cav_enabled
        } else {
            original.cav_enabled
        },
        cav_template: changed.cav_template,
        enabled: if admin {
            changed.enabled
        } else {
            original.enabled
        },
        privacy: changed.privacy,
        trial: if admin { changed.trial } else { original.trial },
    };

    database
        .citizen_change(&cit_query)
        .map_err(|_| ReasonCode::UnableToChangeCitizen)?;

    Ok(())
}

fn citizen_info_vars(
    citizen: &CitizenQuery,
    self_vars: bool,
    admin_vars: bool,
) -> Vec<AWPacketVar> {
    let mut vars = Vec::<AWPacketVar>::new();

    vars.push(AWPacketVar::Uint(VarID::CitizenNumber, citizen.id));
    vars.push(AWPacketVar::String(
        VarID::CitizenName,
        citizen.name.clone(),
    ));
    vars.push(AWPacketVar::String(VarID::CitizenURL, citizen.url.clone()));
    vars.push(AWPacketVar::Byte(VarID::TrialUser, citizen.trial as u8));
    vars.push(AWPacketVar::Byte(
        VarID::CAVEnabled,
        citizen.cav_enabled as u8,
    ));

    if citizen.cav_enabled != 0 {
        vars.push(AWPacketVar::Uint(VarID::CAVTemplate, citizen.cav_template));
    } else {
        vars.push(AWPacketVar::Int(VarID::CAVTemplate, 0));
    }

    if self_vars || admin_vars {
        vars.push(AWPacketVar::Uint(
            VarID::CitizenImmigration,
            citizen.immigration,
        ));
        vars.push(AWPacketVar::Uint(
            VarID::CitizenExpiration,
            citizen.expiration,
        ));
        vars.push(AWPacketVar::Uint(
            VarID::CitizenLastLogin,
            citizen.last_login,
        ));
        vars.push(AWPacketVar::Uint(
            VarID::CitizenTotalTime,
            citizen.total_time,
        ));
        vars.push(AWPacketVar::Uint(VarID::CitizenBotLimit, citizen.bot_limit));
        vars.push(AWPacketVar::Byte(VarID::BetaUser, citizen.beta as u8));
        vars.push(AWPacketVar::Byte(
            VarID::CitizenEnabled,
            citizen.enabled as u8,
        ));
        vars.push(AWPacketVar::Uint(VarID::CitizenPrivacy, citizen.privacy));
        vars.push(AWPacketVar::String(
            VarID::CitizenPassword,
            citizen.password.clone(),
        ));
        vars.push(AWPacketVar::String(
            VarID::CitizenEmail,
            citizen.email.clone(),
        ));
        vars.push(AWPacketVar::String(
            VarID::CitizenPrivilegePassword,
            citizen.priv_pass.clone(),
        ));
        vars.push(AWPacketVar::Uint(
            VarID::CitizenImmigration,
            citizen.immigration,
        ));

        if admin_vars {
            vars.push(AWPacketVar::String(
                VarID::CitizenComment,
                citizen.comment.clone(),
            ));
            vars.push(AWPacketVar::Uint(
                VarID::IdentifyUserIP,
                citizen.last_address,
            ));
        }
    }

    vars
}

fn citizen_from_packet(packet: &AWPacket) -> Result<CitizenQuery, String> {
    let username = packet
        .get_string(VarID::CitizenName)
        .ok_or_else(|| "No citizen name".to_string())?;
    let citizen_id = packet
        .get_uint(VarID::CitizenNumber)
        .ok_or_else(|| "No citizen number".to_string())?;
    let email = packet
        .get_string(VarID::CitizenEmail)
        .ok_or_else(|| "No citizen email".to_string())?;
    let priv_pass = packet
        .get_string(VarID::CitizenPrivilegePassword)
        .ok_or_else(|| "No citizen privilege password".to_string())?;
    let expiration = packet
        .get_uint(VarID::CitizenExpiration)
        .ok_or_else(|| "No citizen expiration".to_string())?;
    let bot_limit = packet
        .get_uint(VarID::CitizenBotLimit)
        .ok_or_else(|| "No citizen bot limit".to_string())?;
    let beta = packet
        .get_uint(VarID::BetaUser)
        .ok_or_else(|| "No citizen beta user".to_string())?;
    let enabled = packet
        .get_uint(VarID::CitizenEnabled)
        .ok_or_else(|| "No citizen enabled".to_string())?;
    let comment = packet
        .get_string(VarID::CitizenComment)
        .ok_or_else(|| "No citizen comment".to_string())?;
    let password = packet
        .get_string(VarID::CitizenPassword)
        .ok_or_else(|| "No citizen password".to_string())?;
    let url = packet
        .get_string(VarID::CitizenURL)
        .ok_or_else(|| "No citizen url".to_string())?;
    let cav_template = packet
        .get_uint(VarID::CAVTemplate)
        .ok_or_else(|| "No citizen cav template".to_string())?;
    let cav_enabled = packet
        .get_uint(VarID::CAVEnabled)
        .ok_or_else(|| "No citizen cav enabled".to_string())?;
    let privacy = packet
        .get_uint(VarID::CitizenPrivacy)
        .ok_or_else(|| "No citizen privacy".to_string())?;
    let trial = packet
        .get_uint(VarID::TrialUser)
        .ok_or_else(|| "No citizen trial".to_string())?;

    Ok(CitizenQuery {
        id: citizen_id,
        changed: 0,
        name: username,
        password,
        email,
        priv_pass,
        comment,
        url,
        immigration: 0,
        expiration,
        last_login: 0,
        last_address: 0,
        total_time: 0,
        bot_limit,
        beta,
        cav_enabled,
        cav_template,
        enabled,
        privacy,
        trial,
    })
}
