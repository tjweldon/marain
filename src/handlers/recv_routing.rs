use std::sync::{Arc, Mutex};

use futures_channel::mpsc::UnboundedSender;
use futures_util::{future, stream::SplitStream, TryStreamExt};
use log::info;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::domain::{types::LockedRoomMap, user::User};


pub async fn recv_routing_handler(
    ws_source: SplitStream<WebSocketStream<TcpStream>>,
    user: Arc<Mutex<User>>,
    command_pipe: UnboundedSender<Message>,
    message_pipe: UnboundedSender<Message>,
    room_map: LockedRoomMap,
) {
    let incoming = ws_source.try_for_each(|msg| {
        if msg.is_close() {
            let rooms = room_map.lock().unwrap();
            let mut members = rooms
                .get(&user.lock().unwrap().room)
                .unwrap()
                .occupants
                .lock()
                .unwrap();
            members.remove(&user.lock().unwrap().id);
        }

        if msg.is_text() {
            let msg_str = msg.to_text().unwrap();
            let chars: Vec<char> = msg_str.chars().collect();
            if chars[0] == '/' {
                info!("forwarding to command worker");
                command_pipe.unbounded_send(msg).unwrap();
            } else {
                info!("forwarding global message worker");
                message_pipe.unbounded_send(msg).unwrap()
            }
        }

        future::ok(())
    });

    incoming.await.unwrap();
}