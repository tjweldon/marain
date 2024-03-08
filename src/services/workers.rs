use std::sync::{Arc, Mutex};

use futures_channel::mpsc::{unbounded, UnboundedReceiver};
use marain_api::prelude::{ClientMsg, ServerMsg};

use crate::{
    domain::{types::LockedRoomMap, user::User},
    handlers::{
        commands::{command_handler, Commands},
        login::SplitSocket,
        messages::global_message_handler,
        recv_routing::recv_routing_handler,
        rooms::room_handler,
    },
};

pub fn spawn_workers(
    user: Arc<Mutex<User>>,
    user_inbox: UnboundedReceiver<ServerMsg>,
    rooms: LockedRoomMap,
    socket: SplitSocket,
) {
    let (msg_sink, msg_source) = unbounded::<ClientMsg>();
    tokio::spawn(global_message_handler(
        socket.sink,
        msg_source,
        rooms.clone(),
        user.clone(),
        user_inbox,
    ));

    //  command messages (incoming)
    let (cmd_sink, cmd_source) = unbounded::<Commands>();
    //  command handling (room state worker)
    let (room_sink, room_source) = unbounded::<Commands>();
    tokio::spawn(command_handler(
        cmd_source,
        room_sink,
        user.clone(),
        rooms.clone(),
    ));
    tokio::spawn(room_handler(
        room_source,
        user.clone(),
        rooms.clone(),
        cmd_sink.clone(),
    ));

    // spawn workers
    tokio::spawn(recv_routing_handler(
        socket.source,
        user.clone(),
        cmd_sink,
        msg_sink,
        rooms.clone(),
    ));
}
