extern crate marain_server;

use env_logger;
use futures_channel::mpsc::unbounded;
use marain_server::{
    domain::commands::Command,
    services::{
        app::App,
        app_gateway::AppGateway,
        login::{create_key_pair, setup_listener, spawn_user_session},
    },
};
use tokio_tungstenite::tungstenite::Result;
use x25519_dalek::{PublicKey, ReusableSecret};
#[macro_use]
extern crate lazy_static;

lazy_static! {
    pub static ref KEY_PAIR: (ReusableSecret, PublicKey) = create_key_pair();
    pub static ref SECRET_KEY: ReusableSecret = KEY_PAIR.0.clone();
    pub static ref PUBLIC_KEY: PublicKey = KEY_PAIR.1;
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let (app_sink, gateway_source) = unbounded::<Command>();
    let (session_sink, session_worker_source) = unbounded::<Command>();
    let app_gateway = AppGateway::init(app_sink, session_worker_source);

    let app = App::init(gateway_source);
    app.run();
    app_gateway.run();
    let listener = setup_listener().await;
    // Create the event loop and TCP listener we'll accept connections on.
    while let Ok((stream, _)) = listener.accept().await {
        match spawn_user_session(
            stream,
            session_sink.clone(),
            (SECRET_KEY.clone(), *PUBLIC_KEY),
        )
        .await
        {
            Err(e) => {
                log::error!("Could not spawn user_session due to error: {e}");
                continue;
            }
            _ => {}
        };
    }

    Ok(())
}
