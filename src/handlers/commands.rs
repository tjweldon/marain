use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::Utc;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;
use log;
use marain_api::prelude::{ServerMsg, ServerMsgBody, Status, Timestamp};
use tokio_tungstenite::tungstenite::Message;

use crate::domain::{
    types::{LockedRoomMap, PeerMap},
    user::User,
};

pub async fn command_handler(
    mut cmd_source: UnboundedReceiver<Message>,
    room_sink: UnboundedSender<Message>,
    user: Arc<Mutex<User>>,
    room: LockedRoomMap,
) {
    while let Some(cmd) = cmd_source.next().await {
        let room_map = room.lock().unwrap();
        let current_room = room_map.get(&user.lock().unwrap().room);

        match current_room {
            Some(rm) => {
                let locked_occupants = rm.occupants.lock();
                prepare_route_command(locked_occupants, &user, cmd, &room_sink);
            }
            None => {
                log::error!(
                    "Unwraped None when trying to access current Room of sender: {:#?}",
                    user
                )
            }
        }
    }
}

fn prepare_route_command(
    locked_occupants: Result<MutexGuard<PeerMap>, PoisonError<MutexGuard<PeerMap>>>,
    user: &Arc<Mutex<User>>,
    cmd: Message,
    room_sink: &UnboundedSender<Message>,
) {
    // Scans the room the user is in and gets their sink for any command with an echoed response.
    // Calls route command with appropriate args.
    match locked_occupants {
        Ok(occupants) => {
            let commander_sink = occupants
                .iter()
                .find_map(|(user_id, (_, c))| {
                    if user_id == &user.lock().unwrap().id {
                        Some(c.clone())
                    } else {
                        None
                    }
                })
                .unwrap();

            route_command(cmd, commander_sink, room_sink, occupants, user);
        }
        Err(e) => {
            log::error!("{e}")
        }
    }
}

#[allow(unused_variables)]
fn route_command(
    cmd: Message,
    commander: UnboundedSender<ServerMsg>,
    room_handler_sink: &UnboundedSender<Message>,
    occupants: MutexGuard<PeerMap>,
    user: &Arc<Mutex<User>>,
) {
    if cmd.is_text() {
        match commander.unbounded_send(ServerMsg {
            status: Status::No("Not implemented".into()),
            timestamp: Timestamp::from(Utc::now()),
            body: ServerMsgBody::Empty,
        }) {
            Err(e) => {
                log::error!("Could not send message to user: {e}")
            }
            _ => {}
        }
        // TODO:
        //let cmd_str: Vec<&str> = cmd.to_text().unwrap_or("").split(" ").collect();
        //match cmd_str[0] {
        //    "/time" => {
        //        let m = Message::Binary(Utc::now().to_string().as_bytes().to_vec());
        //        commander
        //            .unbounded_send(m)
        //            .unwrap_or_else(|e| error!("{}", e));
        //    }
        //    "/mv" => {
        //        info!("forwarding to room handler");
        //        room_handler_sink
        //            .unbounded_send(Message::Binary(cmd_str[1].as_bytes().to_vec()))
        //            .unwrap_or_else(|e| error!("{}", e));
        //    }
        //    "/who" => {
        //        println!("Occupants: {:#?}", occupants);
        //    }
        //    "/crm" => {
        //        println!("Room hash: {}", user.lock().unwrap().room);
        //    }
        //    _ => commander
        //        .unbounded_send(Message::Binary("No such command".as_bytes().to_vec()))
        //        .unwrap_or_else(|e| error!("{}", e)),
        //}
    } else {
        log::error!("Non Text command: {cmd}")
    }
}
