use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};

use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;
use log::{error, info};
use tokio_tungstenite::tungstenite::Message;

use crate::domain::{
    room::Room,
    types::{LockedRoomMap, RoomMap},
    user::User,
    util::hash,
};

pub async fn room_handler(
    mut room_source: UnboundedReceiver<Message>,
    user: Arc<Mutex<User>>,
    room_map: LockedRoomMap,
) {
    while let Some(cmd) = room_source.next().await {
        let room_hash = hash(cmd.to_text().unwrap().to_string());

        let mut rooms: std::sync::MutexGuard<RoomMap> = room_map.lock().unwrap();

        if rooms.contains_key(&room_hash) {
            move_rooms(&rooms, &user, room_hash);
        } else {
            info!("attempting to create room: {} : {}", cmd, room_hash);
            let created = rooms.insert(
                room_hash,
                Room::new(
                    Arc::new(Mutex::new(HashMap::new())),
                    Arc::new(Mutex::new(VecDeque::new())),
                ),
            );
            match created {
                None => move_rooms(&rooms, &user, room_hash),
                Some(_) => {
                    error!("Rooms did not contain key but room was found on insert attempt.")
                }
            }
        }
    }
}

fn move_rooms(rooms: &MutexGuard<RoomMap>, user: &Arc<Mutex<User>>, room_hash: u64) {
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

    // insert the user into the room.
    rooms
        .get(&room_hash)
        .unwrap()
        .occupants
        .lock()
        .unwrap()
        .insert(
            user.lock().unwrap().id.clone(),
            (user.clone(), channel.clone()),
        );
}
