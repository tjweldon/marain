use std::time;

fn main() {
    // this is a contrived commit
    loop {
        std::thread::sleep(time::Duration::from_secs(5));
        println!("THE TIME IS NOW {:?}", time::Instant::now());
    }
}
