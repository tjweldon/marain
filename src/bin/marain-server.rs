extern crate marain_server;

use chrono::Utc;
use env_logger;
use futures_channel::mpsc::{unbounded, UnboundedReceiver};
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use log::info;
use marain_api::prelude::{ClientMsg, ClientMsgBody, ServerMsg, ServerMsgBody, Status, Timestamp};
use marain_server::{
    domain::{room::Room, types::LockedRoomMap, user::User, util::hash},
    handlers::{
        commands::{command_handler, Commands},
        messages::global_message_handler,
        recv_routing::recv_routing_handler,
        rooms::room_handler,
    },
};
use rand_core::OsRng;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    tungstenite::{Message, Result},
    WebSocketStream,
};
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey};

fn getenv(name: &str) -> String {
    match std::env::var(name) {
        Ok(var) => var,
        _ => "".to_string(),
    }
}

fn create_key_pair() -> (EphemeralSecret, PublicKey) {
    let server_secret = EphemeralSecret::random_from_rng(OsRng);
    let server_public = PublicKey::from(&server_secret);

    (server_secret, server_public)
}

fn send_login_fail(mut ws_sink: SplitSink<WebSocketStream<TcpStream>, Message>) {
    tokio::spawn(async move {
        let login_fail = ServerMsg {
            status: Status::JustNo,
            timestamp: Timestamp::from(Utc::now()),
            body: ServerMsgBody::Empty,
        };

        ws_sink
            .send(Message::Binary(bincode::serialize(&login_fail).unwrap()))
            .await
            .unwrap_or(());
        ws_sink.close().await.unwrap_or(());
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let mut port = getenv("MARAIN_PORT");
    if port.len() == 0 {
        port = "8080".to_string();
        log::warn!("Could not find MARAIN_PORT environment variable. Falling back to 8080.");
    }

    let addr = format!("0.0.0.0:{}", port);

    let rooms = LockedRoomMap::new(Mutex::new(HashMap::new()));
    let global_room_hash = hash(String::from("hub"));

    rooms.lock().unwrap().insert(
        global_room_hash,
        Room::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(VecDeque::new())),
        ),
    );
    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let user_addr = stream.peer_addr().unwrap().to_string().clone();
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Error during the websocket handshake occurred");
        info!("Websocket connection from: {}", user_addr,);
        let (mut ws_sink, mut ws_source) = ws_stream.split();

        // Generate a key pair for the server
        let (server_secret, server_public) = create_key_pair();

        // Prepare user fields for login message deserialization.
        let user_id = format!("{:X}", Uuid::new_v4().as_u128());
        let user_name: String;
        let user_public_key: PublicKey;

        // Deserialise the initial login message from a client.
        if let Some(Ok(login_msg)) = ws_source.next().await {
            if let Message::Binary(data) = login_msg {
                if let Ok(ClientMsg {
                    token: None,
                    body: ClientMsgBody::Login(uname, client_public_key), // Unpack a users public key here
                    ..
                }) = bincode::deserialize::<ClientMsg>(&data)
                {
                    user_name = uname;
                    user_public_key = PublicKey::from(client_public_key)
                } else {
                    send_login_fail(ws_sink);
                    continue;
                }
            } else {
                send_login_fail(ws_sink);
                continue;
            }
        } else {
            send_login_fail(ws_sink);
            continue;
        }

        // create & store the user & servers shared secret
        let shared_secret: [u8; 32] = *server_secret.diffie_hellman(&user_public_key).as_bytes();
        let user = Arc::new(Mutex::new(User::new(
            global_room_hash,
            user_id.clone(),
            false,
            user_name.clone(),
            shared_secret,
        )));

        // Login was a huge success, we should congratulate the client.
        let user_inbox = register_user(user.clone(), rooms.clone(), global_room_hash);

        // Include the servers public key with this ServerMsg?
        let login_ok = ServerMsg {
            status: Status::Yes,
            timestamp: Timestamp::from(Utc::now()),
            body: ServerMsgBody::LoginSuccess {
                token: user_id.clone(),
                public_key: *server_public.as_bytes(),
            },
        };
        let serialised: Vec<u8> =
            bincode::serialize(&login_ok).expect("Could not serialize api message");
        ws_sink.send(Message::Binary(serialised)).await.unwrap();
        info!("Registered: {}", user_name.clone());

        // worker initialisation
        // =====================

        //  chat messages (incoming)
        let (msg_sink, msg_source) = unbounded::<ClientMsg>();
        tokio::spawn(global_message_handler(
            ws_sink,
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
            ws_source,
            user.clone(),
            cmd_sink,
            msg_sink,
            rooms.clone(),
        ));
    }

    Ok(())
}

fn register_user(
    user: Arc<Mutex<User>>,
    room: LockedRoomMap,
    room_hash: u64,
) -> UnboundedReceiver<ServerMsg> {
    // Creates an unbounded futures_util::mpsc channel
    // Locks the RoomMap Mutex<HashMap<room_id: ...>>
    // Gets, unwraps, and locks the "hub" room members Mutex<HashMap<usr_id: (user, user_sink)>>
    // Insert a tuple of (User, user_sink) under key of user.id
    // The user is now in the "hub" room and can receive from / broadcast to others in the same room.

    let (user_postbox, user_inbox) = unbounded::<ServerMsg>();
    room.lock()
        .unwrap()
        .get(&room_hash)
        .unwrap()
        .occupants
        .lock()
        .unwrap()
        .insert(
            user.lock().unwrap().id.clone(),
            (user.clone(), user_postbox),
        );

    user_inbox
}
