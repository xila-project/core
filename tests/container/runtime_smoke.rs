use std::{env, fs, process};

fn main() {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("create") => {
            assert_eq!(fs::read("/input/message.txt").unwrap(), b"read-only input");
            assert!(fs::write("/input/should-not-write.txt", b"denied").is_err());
            fs::write("/output/result.txt", b"writable output").unwrap();
            fs::write("/state-marker", b"persisted").unwrap();
        }
        Some("verify") => {
            assert_eq!(fs::read("/input/message.txt").unwrap(), b"read-only input");
            assert_eq!(fs::read("/output/result.txt").unwrap(), b"writable output");
            assert_eq!(fs::read("/state-marker").unwrap(), b"persisted");
        }
        Some("fail") => process::exit(23),
        Some("wait") => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        _ => panic!("expected create, verify, or fail"),
    }
}
