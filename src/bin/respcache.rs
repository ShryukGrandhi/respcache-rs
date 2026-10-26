use std::net::TcpListener;

fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:6380".into());
    let capacity: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let listener = TcpListener::bind(&addr).expect("bind");
    eprintln!("respcache listening on {addr} (max {capacity} keys)");
    respcache::serve(listener, capacity);
}
