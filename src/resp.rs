//! RESP (REdis Serialization Protocol) parsing and encoding for the subset a client sends: arrays of bulk
//! strings, plus inline commands ("PING\r\n").

/// Result of trying to parse one command from the front of a buffer.
#[derive(Debug, PartialEq)]
pub enum Parse {
    /// A complete command and how many bytes it used.
    Command(Vec<Vec<u8>>, usize),
    /// Need more bytes.
    Incomplete,
    /// The input is not valid RESP.
    Error(String),
}

fn line(buf: &[u8], from: usize) -> Option<(&[u8], usize)> {
    let rest = buf.get(from..)?;
    let pos = rest.windows(2).position(|w| w == b"\r\n")?;
    Some((&rest[..pos], from + pos + 2))
}

fn number(s: &[u8]) -> Result<i64, String> {
    std::str::from_utf8(s).ok().and_then(|t| t.parse().ok()).ok_or_else(|| "invalid length".to_string())
}

/// Parse one command; call again on the remaining bytes. Commands may arrive split across reads.
pub fn parse(buf: &[u8]) -> Parse {
    if buf.is_empty() {
        return Parse::Incomplete;
    }
    if buf[0] != b'*' {
        // inline command: space-separated words on one line
        return match line(buf, 0) {
            None => Parse::Incomplete,
            Some((l, used)) => {
                let parts: Vec<Vec<u8>> = l.split(|b| *b == b' ').filter(|p| !p.is_empty()).map(|p| p.to_vec()).collect();
                if parts.is_empty() { Parse::Error("empty command".into()) } else { Parse::Command(parts, used) }
            }
        };
    }
    let (head, mut pos) = match line(buf, 1) {
        None => return Parse::Incomplete,
        Some(x) => x,
    };
    let n = match number(head) {
        Ok(n) if (0..=1024 * 1024).contains(&n) => n as usize,
        Ok(_) => return Parse::Error("bad array length".into()),
        Err(e) => return Parse::Error(e),
    };
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if pos >= buf.len() {
            return Parse::Incomplete;
        }
        if buf[pos] != b'$' {
            return Parse::Error("expected bulk string".into());
        }
        let (len_s, after) = match line(buf, pos + 1) {
            None => return Parse::Incomplete,
            Some(x) => x,
        };
        let len = match number(len_s) {
            Ok(l) if (0..=512 * 1024 * 1024).contains(&l) => l as usize,
            Ok(_) => return Parse::Error("bad bulk length".into()),
            Err(e) => return Parse::Error(e),
        };
        if buf.len() < after + len + 2 {
            return Parse::Incomplete;
        }
        if &buf[after + len..after + len + 2] != b"\r\n" {
            return Parse::Error("bulk string not terminated".into());
        }
        out.push(buf[after..after + len].to_vec());
        pos = after + len + 2;
    }
    Parse::Command(out, pos)
}

pub fn simple(s: &str) -> Vec<u8> { format!("+{s}\r\n").into_bytes() }
pub fn error(s: &str) -> Vec<u8> { format!("-ERR {s}\r\n").into_bytes() }
pub fn integer(n: i64) -> Vec<u8> { format!(":{n}\r\n").into_bytes() }
pub fn nil() -> Vec<u8> { b"$-1\r\n".to_vec() }
pub fn bulk(b: &[u8]) -> Vec<u8> {
    let mut v = format!("${}\r\n", b.len()).into_bytes();
    v.extend_from_slice(b);
    v.extend_from_slice(b"\r\n");
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_array_of_bulk_strings() {
        let b = b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$5\r\nhello\r\n";
        assert_eq!(parse(b), Parse::Command(vec![b"SET".to_vec(), b"k".to_vec(), b"hello".to_vec()], b.len()));
    }

    #[test]
    fn incomplete_at_every_split_point() {
        let b = b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n";
        for cut in 0..b.len() {
            assert_eq!(parse(&b[..cut]), Parse::Incomplete, "cut at {cut}");
        }
    }

    #[test]
    fn two_commands_back_to_back() {
        let b = b"*1\r\n$4\r\nPING\r\n*1\r\n$4\r\nPING\r\n";
        match parse(b) {
            Parse::Command(c, used) => {
                assert_eq!(c, vec![b"PING".to_vec()]);
                assert!(matches!(parse(&b[used..]), Parse::Command(_, _)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn inline_and_errors() {
        assert_eq!(parse(b"PING\r\n"), Parse::Command(vec![b"PING".to_vec()], 6));
        assert!(matches!(parse(b"*1\r\n+OK\r\n"), Parse::Error(_)));
        assert!(matches!(parse(b"*x\r\n"), Parse::Error(_)));
        assert!(matches!(parse(b"*1\r\n$3\r\nabcXY"), Parse::Error(_)));
    }
}
