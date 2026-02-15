#[tokio::main]
async fn main() {
    println!("byo-orchestrator starting");

    // TODO: bind unix socket, accept connections, route APC sequences
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
