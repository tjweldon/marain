use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct NotificationLog {
    pub notifier: String,
    pub timestamp: DateTime<Utc>,
    pub contents: String,
}

impl NotificationLog {
    pub fn new(text: String) -> Self {
        NotificationLog {
            notifier: "SERVER".into(),
            timestamp: Utc::now(),
            contents: text,
        }
    }
}
