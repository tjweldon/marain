use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedSender;
use futures_util::{future, stream::SplitStream, StreamExt};
use log::{self, warn};
use marain_api::prelude::*;
use serde_binary::binary_stream::Endian;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;

use crate::domain::{room::Room, types::LockedRoomMap, user::User};

use super::commands::Commands;

pub async fn recv_routing_handler(
    ws_source: SplitStream<WebSocketStream<TcpStream>>,
    user: Arc<Mutex<User>>,
    command_pipe: UnboundedSender<Commands>,
    message_pipe: UnboundedSender<ClientMsg>,
    room_map: LockedRoomMap,
) {
    _ = ws_source
        .for_each(|msg_maybe| {
            match msg_maybe {
                Ok(msg) => {
                    log::info!("MESSAGE ARRIVED {msg:?}");
                    if msg.is_close() {
                        remove_user(room_map.clone(), user.clone());
                    } else if msg.is_binary() {
                        let msg_bytes = msg.into_data();
                        match serde_binary::from_vec::<ClientMsg>(msg_bytes, Endian::Little) {
                            Err(e) => log::warn!("Unrecognised message from client: {e}"),
                            Ok(cm) => match cm {
                                ClientMsg {
                                    token: Some(_),
                                    body: ClientMsgBody::SendToRoom { .. },
                                    ..
                                } => {
                                    message_pipe.unbounded_send(cm).unwrap();
                                    log::info!("published chat message")
                                }
                                ClientMsg {
                                    token: Some(_),
                                    body: ClientMsgBody::GetTime,
                                    ..
                                } => {
                                    command_pipe.unbounded_send(Commands::GetTime).unwrap();
                                    log::info!("Pushed Time command to handler")
                                }
                                ClientMsg {
                                    token: Some(id),
                                    body: ClientMsgBody::Move { target },
                                    ..
                                } => {
                                    command_pipe
                                        .unbounded_send(Commands::Move {
                                            user_id: id,
                                            target,
                                        })
                                        .unwrap();

                                    log::info!("Pushed move command to handler");
                                }

                                _ => {}
                            },
                        };
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
