extern crate marain_server;

use env_logger;
use marain_server::{
    handlers::login::{
        handle_initial_connection, login_handshake, setup_listener, setup_rooms,
    },
    services::workers::spawn_workers,
};

use tokio_tungstenite::tungstenite::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let (rooms, global_room_hash) = setup_rooms();

    // Create the event loop and TCP listener we'll accept connections on.
    let listener = setup_listener().await;

    while let Ok((stream, _)) = listener.accept().await {
        let split_socket = handle_initial_connection(stream).await;

        let success = match login_handshake(global_room_hash, rooms.clone(), split_socket).await {
            Ok(ls) => ls,
            Err(e) => {
                log::info!("{e:?}");
                continue;
            }
        };

        spawn_workers(
            success.user.clone(),
            success.user_inbox,
            success.rooms.clone(),
            success.socket,
        )
    }

    Ok(())
}
