use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedReceiver;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use log::{self, error};
use marain_api::prelude::{ChatMsg, ClientMsg, ServerMsg, ServerMsgBody, Status};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::{chat_log::MessageLog, types::LockedRoomMap, user::User};

pub async fn global_message_handler(
    mut ws_sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    mut message: UnboundedReceiver<ClientMsg>,
    room_map: LockedRoomMap,
    user: Arc<Mutex<User>>,
    mut user_inbox: UnboundedReceiver<ServerMsg>,
) {
    // Extract the user id and name for read only use for the lifetime of this worker.
    // Exit gracefully with error log if the lock cannot be acquired.
    let Some((user_id, user_name)) = (match user.lock() {
        Ok(usr) => Some((usr.id.clone(), usr.name.clone())),
        _ => None,
    }) else {
        log::error!(
            "Failed to get the name and id for the user to which this worker task is attached. exiting."
        );
        return;
    };

    let user_id: &str = &user_id;
    'main_loop: loop {
        tokio::select! {
            msg_from_user = message.next() => {
                let Some(ref msg_ref) = msg_from_user else {
                    break
                };
                let msg: ClientMsg = msg_ref.clone();
                let client_user_id = msg.clone().token.unwrap_or("".into()).clone();
                if !client_user_id.eq(user_id) {
                    log::warn!(
                        "The user id {} from the inbound message did not match the current user: {}.",
                        client_user_id,
                        user_id
                    );
                    continue 'main_loop;
                }


                let mut rooms = room_map.lock().unwrap();
                let user_room_id = user.lock().unwrap().room;
                if let Some(user_room) = rooms.get_mut(&user_room_id) {

                    if let Some(msg_log) = MessageLog::from_client_msg(msg.clone(), &user_name) {
                        user_room.new_message(msg_log.clone());
                        user_room.remove_oldest_message();

                        // the broadcast message is the same for every receipient
                        let broadcast_msg = ServerMsg {
                            status: Status::Yes,
                            timestamp: msg.timestamp.clone(),
                            body: ServerMsgBody::ChatRecv {
                                direct: false,
                                chat_msg: ChatMsg {
                                    sender: user_name.clone(),
                                    timestamp: msg.timestamp.clone(),
                                    content: msg_log.contents.clone()
                                }
                            }
                        };

                        for receipient in user_room.get_recipients_except(user_id) {
                            receipient
                                .unbounded_send(broadcast_msg.clone())
                                .unwrap_or_else(|e| error!("{}", e))
                        }
                    }
                }
            }

            msg_to_usr = user_inbox.next() => {
                match msg_to_usr {
                    Some(m) => {
                        match serde_json::to_string(&m) {
                            Ok(ser) => ws_sink.send(Message::Text(ser)).await.unwrap_or_else(|e| error!("{}", e)),
                            Err(e) => {
                                log::error!("Could not broadcast: {e}");
                            }
                        }
                    }
                    None => {}
                }
            }
        }
    }
}
