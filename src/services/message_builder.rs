use chrono::Utc;

use marain_api::prelude::{ChatMsg, ServerMsg, ServerMsgBody, Status, Timestamp};

use sphinx::prelude::{cbc_encode, get_rng};
use tokio_tungstenite::tungstenite::Message;

use super::{user::User, chat_log::MessageLog};

use anyhow::{anyhow, Result};

pub struct SocketSendAdaptor;

impl SocketSendAdaptor {
    pub fn serialized_server_msg(s: ServerMsg) -> Result<Vec<u8>> {
        let serialized = match bincode::serialize(&s) {
            Ok(ser) => ser,
            Err(e) => {
                return Err(
                    anyhow!(
                "Bincode::serialize failed with Error: {e:?}. Failed serializing ServerMsg: {s:?}"),
                );
            }
        };

        Ok(serialized)
    }

    fn encrypt_message(key: &[u8; 32], data: Vec<u8>) -> Result<Message> {
        let rng = get_rng();
        match cbc_encode(key.to_vec(), data, rng) {
            Ok(enc) => Ok(Message::Binary(enc)),
            Err(e) => Err(anyhow!("{e:?}")),
        }
    }

    pub fn on_login_success(token: String, public_key: [u8; 32]) -> Result<Message> {
        let server_msg = ServerMsgFactory::build_login_success_server_msg(token, public_key);
        let serialized = SocketSendAdaptor::serialized_server_msg(server_msg)?;
        Ok(Message::Binary(serialized))
    }

    pub fn prepare_send_msg_log(msg: MessageLog, user: &User, key: &[u8; 32]) -> Result<Message> {
        let server_msg = ServerMsgFactory::build_msg_log_server_msg(msg, user);
        let serialized = SocketSendAdaptor::serialized_server_msg(server_msg)?;
        let encrypted = SocketSendAdaptor::encrypt_message(key, serialized)?;
        Ok(encrypted)
    }

    pub fn prepare_send_time(key: &[u8; 32], t: Timestamp) -> Result<Message> {
        let server_msg = ServerMsgFactory::build_time_server_msg(t);
        let serialized = SocketSendAdaptor::serialized_server_msg(server_msg)?;
        let encrypted = SocketSendAdaptor::encrypt_message(key, serialized)?;
        Ok(encrypted)
    }
}

pub struct ServerMsgFactory;

impl ServerMsgFactory {
    fn build_login_success_server_msg(token: String, public_key: [u8; 32]) -> ServerMsg {
        ServerMsg {
            status: Status::Yes,
            timestamp: Timestamp::from(Utc::now()),
            body: ServerMsgBody::LoginSuccess { token, public_key },
        }
    }

    fn build_msg_log_server_msg(msg: MessageLog, user: &User) -> ServerMsg {
        ServerMsg {
            status: Status::Yes,
            timestamp: msg.timestamp.into(),
            body: ServerMsgBody::ChatRecv {
                direct: false,
                chat_msg: ChatMsg {
                    sender: user.name.clone(),
                    timestamp: msg.timestamp.into(),
                    content: msg.contents.clone(),
                },
            },
        }
    }

    fn build_time_server_msg(time: Timestamp) -> ServerMsg {
        ServerMsg {
            status: Status::Yes,
            timestamp: Timestamp::from(time),
            body: ServerMsgBody::Empty,
        }
    }
}
