use chrono::Utc;
use futures_channel::mpsc::UnboundedSender;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use log::info;
use marain_api::prelude::{ClientMsg, ClientMsgBody, ServerMsg, ServerMsgBody, Status, Timestamp};

use rand_core::OsRng;

use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use anyhow::{anyhow, Result};
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey};

use super::{
    commands::Command, message_builder::SocketSendAdaptor, user::User, user_session::SessionWorker,
};

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

pub struct SplitSocket {
    pub sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    pub source: SplitStream<WebSocketStream<TcpStream>>,
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
    user: User,
    mut sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    source: SplitStream<WebSocketStream<TcpStream>>,
    server_public_key: PublicKey,
    gateway_sink: UnboundedSender<Command>,
) -> Result<SessionWorker> {
    let login_success_response =
        SocketSendAdaptor::on_login_success(user.id.clone(), server_public_key.to_bytes())?;

    match sink.send(login_success_response).await {
        Err(e) => {
            return Err(anyhow!(
                "Failed to send successful login response: Error: {e}"
            ));
        }
        _ => {}
    };

    let session_worker = SessionWorker::new(user, gateway_sink, sink, source);

    Ok(session_worker)
}

/// handle_login_attempt consumes a deserialised login message and takes care of key shared
/// secret management.
pub async fn handle_login_attempt(
    login_msg: ClientMsg,
    socket_sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    socket_source: SplitStream<WebSocketStream<TcpStream>>,
    gateway_sink: UnboundedSender<Command>,
    server_secret: EphemeralSecret,
    server_public_key: PublicKey,
) -> Result<SessionWorker> {
    // Deserialise the initial login message from a client.
    if let ClientMsg {
        token: None,
        body: ClientMsgBody::Login(uname, client_public_key), // Unpack a users public key here
        ..
    } = login_msg
    {
        let name = uname;
        let public_key = PublicKey::from(client_public_key);
        let id = format!("{:X}", Uuid::new_v4().as_u128());

        let shared_secret = *server_secret.diffie_hellman(&public_key).as_bytes();

        on_login_success(
            User::new(id, name, shared_secret),
            socket_sink,
            socket_source,
            server_public_key,
            gateway_sink,
        )
        .await
    } else {
        on_login_failed(socket_sink);
        Err(anyhow!(
            "Login failed: Could not deserialize login message".to_string(),
        ))
    }
}

/// handle_client_initialisation covers failure modes where the server
/// does not receive a well formed initial message from the client on
/// establishing the websocket connection.
pub async fn handle_client_initiation(
    mut socket_source: SplitStream<WebSocketStream<TcpStream>>,
    sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    server_secret: EphemeralSecret,
    server_public_key: PublicKey,
    gateway_sink: UnboundedSender<Command>,
) -> Result<SessionWorker> {
    match socket_source.next().await {
        Some(Ok(Message::Binary(data))) => {
            let deserialized: ClientMsg = match bincode::deserialize(&data[..]) {
                Ok(m) => m,
                Err(e) => {
                    let err_msg =
                        format!("Error during user client initiation, unrecognised message: {e}");
                    log::error!("{err_msg}");
                    return Err(anyhow!("{err_msg}"));
                }
            };

            return handle_login_attempt(
                deserialized,
                sink,
                socket_source,
                gateway_sink,
                server_secret,
                server_public_key,
            )
            .await;
        }
        _ => {
            log::error!("Could not read inbound connection from user");
            return Err(anyhow!("Could not read inbound connection from user"));
        }
    }
}

pub async fn login_handshake(
    socket: SplitSocket,
    gateway_sink: UnboundedSender<Command>,
) -> Result<SessionWorker> {
    // Generate a key pair for the server
    let (server_secret, server_public) = create_key_pair();
    let SplitSocket { sink, source } = socket;

    handle_client_initiation(source, sink, server_secret, server_public, gateway_sink).await
}

pub async fn spawn_user_session(stream: TcpStream, gateway_sink: UnboundedSender<Command>) -> Result<()>{
    let split_socket = handle_initial_connection(stream).await;
    let mut user_session = login_handshake(split_socket, gateway_sink).await?;
    tokio::spawn(async move {
        if let Err(e) = user_session.run().await {
            log::error!("User session quit unexpectedly with error: {e}");
        }
    });

    Ok(())
}
