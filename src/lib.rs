//! respcache: an in-memory LRU cache server speaking a subset of the Redis protocol.
//! Supported commands: PING, ECHO, GET, SET (with EX / PX), DEL, EXISTS, INCR, EXPIRE, TTL, DBSIZE.

pub mod cache;
pub mod resp;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cache::Cache;

/// Execute one command against the cache and return the RESP reply.
pub fn execute(cache: &Mutex<Cache>, cmd: &[Vec<u8>], now: Instant) -> Vec<u8> {
    let name = String::from_utf8_lossy(&cmd[0]).to_ascii_uppercase();
    let args = &cmd[1..];
    let mut c = cache.lock().unwrap();
    let wrong = |n: &str| resp::error(&format!("wrong number of arguments for '{n}'"));
    match name.as_str() {
        "PING" => match args.len() { 0 => resp::simple("PONG"), 1 => resp::bulk(&args[0]), _ => wrong("ping") },
        "ECHO" if args.len() == 1 => resp::bulk(&args[0]),
        "GET" if args.len() == 1 => c.get(&args[0], now).map(|v| resp::bulk(&v)).unwrap_or_else(resp::nil),
        "SET" if args.len() == 2 || args.len() == 4 => {
            let ttl = if args.len() == 4 {
                let opt = String::from_utf8_lossy(&args[2]).to_ascii_uppercase();
                let n: u64 = match std::str::from_utf8(&args[3]).ok().and_then(|s| s.parse().ok()) {
                    Some(n) if n > 0 => n,
                    _ => return resp::error("invalid expire time"),
                };
                match opt.as_str() {
                    "EX" => Some(Duration::from_secs(n)),
                    "PX" => Some(Duration::from_millis(n)),
                    _ => return resp::error("syntax error"),
                }
            } else {
                None
            };
            c.set(&args[0], args[1].clone(), ttl, now);
            resp::simple("OK")
        }
        "DEL" if !args.is_empty() => resp::integer(args.iter().filter(|k| c.delete(k)).count() as i64),
        "EXISTS" if !args.is_empty() => resp::integer(args.iter().filter(|k| c.exists(k, now)).count() as i64),
        "INCR" if args.len() == 1 => {
            let cur = c.get(&args[0], now).unwrap_or_else(|| b"0".to_vec());
            match std::str::from_utf8(&cur).ok().and_then(|s| s.parse::<i64>().ok()).and_then(|n| n.checked_add(1)) {
                Some(n) => {
                    c.set(&args[0], n.to_string().into_bytes(), None, now);
                    resp::integer(n)
                }
                None => resp::error("value is not an integer or out of range"),
            }
        }
        "EXPIRE" if args.len() == 2 => match std::str::from_utf8(&args[1]).ok().and_then(|s| s.parse::<u64>().ok()) {
            Some(s) => resp::integer(c.expire(&args[0], Duration::from_secs(s), now) as i64),
            None => resp::error("value is not an integer or out of range"),
        },
        "TTL" if args.len() == 1 => resp::integer(c.ttl(&args[0], now)),
        "DBSIZE" if args.is_empty() => resp::integer(c.len() as i64),
        "ECHO" | "GET" | "SET" | "DEL" | "EXISTS" | "INCR" | "EXPIRE" | "TTL" | "DBSIZE" => wrong(&name.to_lowercase()),
        _ => resp::error(&format!("unknown command '{}'", name.to_lowercase())),
    }
}

fn serve_client(mut stream: TcpStream, cache: Arc<Mutex<Cache>>) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        loop {
            match resp::parse(&buf) {
                resp::Parse::Incomplete => break,
                resp::Parse::Error(e) => {
                    let _ = stream.write_all(&resp::error(&format!("Protocol error: {e}")));
                    return;
                }
                resp::Parse::Command(cmd, used) => {
                    let reply = execute(&cache, &cmd, Instant::now());
                    if stream.write_all(&reply).is_err() {
                        return;
                    }
                    buf.drain(..used);
                }
            }
        }
    }
}

/// Accept connections forever, one thread per client.
pub fn serve(listener: TcpListener, capacity: usize) {
    let cache = Arc::new(Mutex::new(Cache::new(capacity)));
    for stream in listener.incoming().flatten() {
        let cache = Arc::clone(&cache);
        thread::spawn(move || serve_client(stream, cache));
    }
}
