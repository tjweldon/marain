use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;
use log::info;
use marain_api::prelude::{ChatMsg, ServerMsg, Timestamp};

use crate::domain::{
    room::Room,
    types::{LockedRoomMap, RoomMap},
    user::User,
    util::hash,
};

use super::commands::Commands;

pub async fn room_handler(
    mut room_source: UnboundedReceiver<Commands>,
    user: Arc<Mutex<User>>,
    room_map: LockedRoomMap,
    cmd_sink: UnboundedSender<Commands>,
) {
    let worker_user = user.lock().unwrap().id.clone();
    while let Some(cmd) = room_source.next().await {
        match cmd {
            Commands::Move {
                user_id: requesting_user,
                target,
            } => {
                if requesting_user != user.lock().unwrap().id {
                    log::error!("Received a command from a user not for this worker: Requesting User ID: {requesting_user}, Workers User: {worker_user}");
                    continue;
                }

                let room_hash = hash(target.clone());
                let mut rooms: std::sync::MutexGuard<RoomMap> = room_map.lock().unwrap();

                if rooms.contains_key(&room_hash) {
                    move_rooms(&rooms, &user, room_hash.clone(), cmd_sink.clone());
                } else {
                    info!("attempting to create room: {:?} : {}", target, room_hash);
                    let created = rooms.insert(
                        room_hash,
                        Room::new(
                            Arc::new(Mutex::new(HashMap::new())),
                            Arc::new(Mutex::new(VecDeque::new())),
                            target,
                            room_hash,
                        ),
                    );
                    match created {
                        None => move_rooms(&rooms, &user, room_hash, cmd_sink.clone()),
                        Some(_) => {
                            log::error!(
                                "Rooms did not contain key but room was found on insert attempt."
                            );
                            continue;
                        }
                    }
                }
            }
            _ => {
                log::warn!("Upstream channel closed.");
                break;
            }
        }
    }
}

fn move_rooms(
    rooms: &MutexGuard<RoomMap>,
    user: &Arc<Mutex<User>>,
    room_hash: u64,
    cmd_sink: UnboundedSender<Commands>,
) {
    info!(
        "Moving user_id: {} to {}",
        user.lock().unwrap().id,
        room_hash
    );

    // find the user in the current room and remove them.
    let (_usr_id, (_u, channel)) = rooms
        .iter()
        .find_map(|(_, room)| {
            room.occupants
                .lock()
                .unwrap()
                .remove_entry(&user.lock().unwrap().id)
        })
        .unwrap();

    // update user with new room id, reset chat history flag.
    user.lock().unwrap().room = room_hash;
    user.lock().unwrap().up_to_date = false;
    let room = rooms.get(&room_hash).unwrap();

    let mut occupants: MutexGuard<
        '_,
        HashMap<
            String,
            (
                Arc<Mutex<User>>,
                UnboundedSender<marain_api::prelude::ServerMsg>,
            ),
        >,
    > = room.occupants.lock().unwrap();
    occupants.insert(
        user.lock().unwrap().id.clone(),
        (user.clone(), channel.clone()),
    );

    push_destination_room_data(room, occupants, cmd_sink.clone())
}

fn push_destination_room_data(
    room: &Room,
    occupants: MutexGuard<HashMap<String, (Arc<Mutex<User>>, UnboundedSender<ServerMsg>)>>,
    cmd_sink: UnboundedSender<Commands>,
) {
    let chat_messages: Vec<ChatMsg> = room
        .chat_log
        .lock()
        .unwrap()
        .iter()
        .map(|m| ChatMsg {
            sender: m.username.clone(),
            timestamp: Timestamp::from(m.timestamp),
            content: m.contents.clone(),
        })
        .collect();
    let room_data = Commands::SendRoomData {
        messages: chat_messages,
        occupants: occupants
            .values()
            .map(|(o, _)| o.lock().unwrap().name.clone())
            .collect(),
    };
    cmd_sink.unbounded_send(room_data).unwrap();
}
