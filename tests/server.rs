//! End-to-end: start the server on an ephemeral port and talk RESP over TCP.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn start(capacity: usize) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    thread::spawn(move || respcache::serve(l, capacity));
    addr
}

fn cmd(parts: &[&str]) -> Vec<u8> {
    let mut v = format!("*{}\r\n", parts.len()).into_bytes();
    for p in parts {
        v.extend(format!("${}\r\n{}\r\n", p.len(), p).bytes());
    }
    v
}

fn roundtrip(s: &mut TcpStream, parts: &[&str]) -> String {
    s.write_all(&cmd(parts)).unwrap();
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[test]
fn set_get_del_incr_over_tcp() {
    let addr = start(100);
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(roundtrip(&mut s, &["PING"]), "+PONG\r\n");
    assert_eq!(roundtrip(&mut s, &["SET", "k", "hello"]), "+OK\r\n");
    assert_eq!(roundtrip(&mut s, &["GET", "k"]), "$5\r\nhello\r\n");
    assert_eq!(roundtrip(&mut s, &["INCR", "n"]), ":1\r\n");
    assert_eq!(roundtrip(&mut s, &["INCR", "n"]), ":2\r\n");
    assert_eq!(roundtrip(&mut s, &["INCR", "k"]), "-ERR value is not an integer or out of range\r\n");
    assert_eq!(roundtrip(&mut s, &["DEL", "k", "missing"]), ":1\r\n");
    assert_eq!(roundtrip(&mut s, &["GET", "k"]), "$-1\r\n");
    assert_eq!(roundtrip(&mut s, &["SET", "t", "v", "EX", "100"]), "+OK\r\n");
    assert_eq!(roundtrip(&mut s, &["TTL", "t"]), ":100\r\n");
    assert!(roundtrip(&mut s, &["NOPE"]).starts_with("-ERR unknown command"));
}

#[test]
fn command_split_across_writes() {
    let addr = start(100);
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let c = cmd(&["SET", "split", "value"]);
    s.write_all(&c[..7]).unwrap();
    thread::sleep(Duration::from_millis(50));
    s.write_all(&c[7..]).unwrap();
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"+OK\r\n");
}

#[test]
fn lru_capacity_over_tcp() {
    let addr = start(2);
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    roundtrip(&mut s, &["SET", "a", "1"]);
    roundtrip(&mut s, &["SET", "b", "2"]);
    roundtrip(&mut s, &["GET", "a"]);
    roundtrip(&mut s, &["SET", "c", "3"]);
    assert_eq!(roundtrip(&mut s, &["GET", "b"]), "$-1\r\n");
    assert_eq!(roundtrip(&mut s, &["DBSIZE"]), ":2\r\n");
}
