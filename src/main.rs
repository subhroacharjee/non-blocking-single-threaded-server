use std::io::Result;

use non_blocking_poll_server::BlockingTcpListener;

fn main() -> Result<()> {
    let mut server = BlockingTcpListener::new("localhost:8888".to_string());
    server.initalize();
    server.event_loop()
}
