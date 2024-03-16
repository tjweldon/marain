use chrono::Utc;
use futures_channel::mpsc::{unbounded, UnboundedReceiver};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use log::info;
use marain_api::prelude::{
    ClientMsg, ClientMsgBody, MarainError, ServerMsg, ServerMsgBody, Status, Timestamp,
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

use crate::domain::{room::Room, types::LockedRoomMap, user::User, util::hash};

pub fn getenv(name: &str) -> String {
    match std::env::var(name) {
        Ok(var) => var,
        _ => "".to_string(),
    }
}

pub fn create_key_pair() -> (EphemeralSecret, PublicKey) {
    let server_secret = EphemeralSecret::random_from_rng(OsRng);
    let server_public = PublicKey::from(&server_secret);

    (server_secret, server_public)
}

pub fn setup_rooms() -> (LockedRoomMap, u64) {
    let rooms = LockedRoomMap::new(Mutex::new(HashMap::new()));
    let global_room_hash = hash(String::from("hub"));

    rooms.lock().unwrap().insert(
        global_room_hash,
        Room::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(VecDeque::new())),
            "hub".into(),
            global_room_hash,
        ),
    );
    (rooms, global_room_hash)
}

pub async fn setup_listener() -> TcpListener {
    let mut port = getenv("MARAIN_PORT");
    if port.len() == 0 {
        port = "8080".to_string();
        log::warn!("Could not find MARAIN_PORT environment variable. Falling back to 8080.");
    }
    let addr = format!("0.0.0.0:{}", port);
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr);
    listener
}

pub struct SplitSocket {
    pub sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    pub source: SplitStream<WebSocketStream<TcpStream>>,
}

pub async fn handle_initial_connection(stream: TcpStream) -> SplitSocket {
    let user_addr = stream.peer_addr().unwrap().to_string();
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");
    info!("Websocket connection from: {}", user_addr,);
    let (ws_sink, ws_source) = ws_stream.split();

    SplitSocket {
        sink: ws_sink,
        source: ws_source,
    }
}

pub fn on_login_failed(mut socket_sink: SplitSink<WebSocketStream<TcpStream>, Message>) {
    tokio::spawn(async move {
        let login_fail = ServerMsg {
            status: Status::JustNo,
            timestamp: Timestamp::from(Utc::now()),
            body: ServerMsgBody::Empty,
        };

        socket_sink
            .send(Message::Binary(bincode::serialize(&login_fail).unwrap()))
            .await
            .unwrap_or(());
        socket_sink.close().await.unwrap_or(());
    });
}

pub async fn on_login_success(
    user_id: String,
    server_public: PublicKey,
    mut socket: SplitSocket,
) -> Result<SplitSocket, MarainError> {
    let login_ok = ServerMsg {
        status: Status::Yes,
        timestamp: Timestamp::from(Utc::now()),
        body: ServerMsgBody::LoginSuccess {
            token: user_id.clone(),
            public_key: *server_public.as_bytes(),
        },
    };

    let serialised: Vec<u8> = match bincode::serialize(&login_ok) {
        Ok(ser) => ser,
        Err(e) => {
            let _ = socket.sink.close().await.unwrap_or(());
            return Err(MarainError::LoginFail(format!("{e:?}")));
        }
    };

    socket
        .sink
        .send(Message::Binary(serialised))
        .await
        .unwrap_or(());
    Ok(socket)
}

pub async fn handle_login_attempt(
    mut split_socket: SplitSocket,
) -> Result<(String, String, PublicKey, SplitSocket), MarainError> {
    // Prepare user fields for login message deserialization.
    let user_id = format!("{:X}", Uuid::new_v4().as_u128());
    let user_name: String;
    let user_public_key: PublicKey;

    // Deserialise the initial login message from a client.
    if let Some(Ok(login_msg)) = split_socket.source.next().await {
        if let Message::Binary(data) = login_msg {
            if let Ok(ClientMsg {
                token: None,
                body: ClientMsgBody::Login(uname, client_public_key), // Unpack a users public key here
                ..
            }) = bincode::deserialize::<ClientMsg>(&data)
            {
                user_name = uname;
                user_public_key = PublicKey::from(client_public_key);
                Ok((user_id, user_name, user_public_key, split_socket))
            } else {
                on_login_failed(split_socket.sink);
                Err(MarainError::LoginFail(
                    "Login failed: Could not deserialize login message".to_string(),
                ))
            }
        } else {
            on_login_failed(split_socket.sink);
            Err(MarainError::LoginFail(format!("Login failed: Incorrect message format from client. Expected Message::Binary. Got: {login_msg:?}")))
        }
    } else {
        on_login_failed(split_socket.sink);
        Err(MarainError::LoginFail(format!(
            "Login failed: Downstream connection closed unexpectedly."
        )))
    }
}

pub fn register_user(
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

pub fn setup_user(
    global_room_hash: u64,
    user_id: String,
    user_name: String,
    user_public_key: PublicKey,
    rooms: LockedRoomMap,
    server_secret: EphemeralSecret,
) -> (Arc<Mutex<User>>, UnboundedReceiver<ServerMsg>) {
    // create & store the user & servers shared secret
    let shared_secret: [u8; 32] = *server_secret.diffie_hellman(&user_public_key).as_bytes();

    let user = Arc::new(Mutex::new(User::new(
        global_room_hash,
        user_id,
        false,
        user_name.clone(),
        shared_secret,
    )));

    let user_inbox = register_user(user.clone(), rooms.clone(), global_room_hash);
    info!("Registered: {}", user_name);
    (user, user_inbox)
}

pub struct UserSession {
    pub user: Arc<Mutex<User>>,
    pub user_inbox: UnboundedReceiver<ServerMsg>,
    pub rooms: LockedRoomMap,
    pub socket: SplitSocket,
}

pub async fn login_handshake(
    global_room_hash: u64,
    rooms: LockedRoomMap,
    socket: SplitSocket,
) -> Result<UserSession, MarainError> {
    let (user_id, user_name, user_public_key, split_socket) = handle_login_attempt(socket).await?;

    // Generate a key pair for the server
    let (server_secret, server_public) = create_key_pair();

    let (user, user_inbox) = setup_user(
        global_room_hash,
        user_id.clone(),
        user_name.clone(),
        user_public_key,
        rooms.clone(),
        server_secret,
    );

    let split_socket =
        on_login_success(user_id.clone(), server_public.clone(), split_socket).await?;

    Ok(UserSession {
        user,
        user_inbox,
        rooms,
        socket: split_socket,
    })
}
