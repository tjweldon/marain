use std::fmt::Display;

use chrono::{DateTime, Utc};
use marain_api::prelude::{ClientMsg, ClientMsgBody};

#[derive(Debug, Clone)]
pub struct MessageLog {
    pub username: String,
    pub timestamp: DateTime<Utc>,
    pub contents: String,
}

impl MessageLog {
    pub fn from_client_msg(client_msg: ClientMsg, username: &str) -> Option<Self> {
        match client_msg.body {
            ClientMsgBody::SendToRoom { contents } => Some(MessageLog {
                username: username.into(),
                timestamp: match client_msg.timestamp.into() {
                    Some(ts) => ts,
                    // just use now if we can't parse the raw ts
                    None => Utc::now(),
                },
                contents,
            }),
            _ => None,
        }
    }
}

impl Display for MessageLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[ {} | {} ]: {}",
            self.username,
            self.timestamp.format("%H-%M-%S"),
            self.contents
        )
    }
}
