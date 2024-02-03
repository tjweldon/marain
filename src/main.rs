use chrono::{Timelike, Utc};
use std::time;

fn main() {
    // this is a contrived commit
    loop {
        std::thread::sleep(time::Duration::from_secs(5));
        let now = Utc::now();
        let (is_pm, hour) = now.hour12();
        println!(
            "THE TIME IS NOW {:02}:{:02}:{:02} {}",
            hour,
            now.minute(),
            now.second(),
            if is_pm { "PM" } else { "AM" }
        );
    }
}
