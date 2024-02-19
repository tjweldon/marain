use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedSender;
use futures_util::{future, stream::SplitStream, StreamExt};
use log::{self, warn};
use marain_api::prelude::*;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::{room::Room, types::LockedRoomMap, user::User};

pub async fn recv_routing_handler(
    ws_source: SplitStream<WebSocketStream<TcpStream>>,
    user: Arc<Mutex<User>>,
    command_pipe: UnboundedSender<Message>,
    message_pipe: UnboundedSender<ClientMsg>,
    room_map: LockedRoomMap,
) {
    _ = ws_source
        .for_each(|msg_maybe| {
            match msg_maybe {
                Ok(msg) => {
                    if msg.is_close() {
                        remove_user(room_map.clone(), user.clone());
                    } else if msg.is_text() {
                        let msg_str = msg.to_text().unwrap();
                        let chars: Vec<char> = msg_str.chars().collect();
                        if chars[0] == '/' {
                            log::info!("forwarding to command worker");
                            command_pipe.unbounded_send(msg).unwrap();
                        } else {
                            match serde_json::from_str::<ClientMsg>(msg_str) {
                                Err(_) => warn!("Unrecognised message from client: {msg_str}"),
                                Ok(cm) => match cm {
                                    ClientMsg {
                                        token: Some(_),
                                        body: ClientMsgBody::SendToRoom { .. },
                                        ..
                                    } => {
                                        message_pipe.unbounded_send(cm).unwrap();
                                        log::info!("published chat message")
                                    }
                                    _ => {}
                                },
                            };
                        }
                    }
                }
                Err(e) => {
                    remove_user(room_map.clone(), user.clone());
                    warn!("Disconnected user due to upstream error: {e}");
                }
            }

            future::ready(())
        })
        .await;
}

fn remove_user(
    room_map: Arc<Mutex<std::collections::HashMap<u64, crate::domain::room::Room>>>,
    user: Arc<Mutex<User>>,
) {
    let rooms = room_map.lock().unwrap();
    let empty = Room::default();
    let mut members = rooms
        .get(&user.lock().unwrap().room)
        .unwrap_or(&empty)
        .occupants
        .lock()
        .expect("Something else broke. ‾\\(`>`)/‾");
    members.remove(&user.lock().unwrap().id);
}
