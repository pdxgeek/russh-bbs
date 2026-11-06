//! The `ClamAV` adapter for the scan hook (M4-F2): the `INSTREAM` protocol of `clamd`, over TCP or
//! a unix socket, behind `[files] scan = "clamd"`.
//!
//! Protocol (clamd(8), "INSTREAM"): send `zINSTREAM\0`, then the content as chunks, each a
//! 4-byte big-endian length and that many bytes, then a zero-length chunk; clamd answers one
//! NUL-terminated line: `stream: OK`, `stream: <signature> FOUND`, or `<text> ERROR`. A reply of
//! any other shape, a connection that drops, a timeout, or a size-limit error is
//! [`ScanOutcome::Failed`]: the file stays in quarantine and is never auto-approved.
//!
//! The daemon is trusted to be on a private network or a local socket (the stream is not
//! encrypted); the reply text is untrusted and is sanitized by core before it is stored.

use std::path::PathBuf;
use std::time::Duration;

use russh_bbs_core::files::{NoScan, ScanHook, ScanOutcome};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};

/// Bytes per `INSTREAM` chunk.
const CHUNK: usize = 64 * 1024;
/// Longest reply line read; a longer one is a protocol error.
const MAX_REPLY: usize = 4 * 1024;
/// How long a connection attempt may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Where `clamd` listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClamdTarget {
    /// `host:port`.
    Tcp(String),
    /// A unix socket path.
    Unix(PathBuf),
}

/// A scanner that streams each upload to `clamd`.
#[derive(Debug, Clone)]
pub struct ClamdScanner {
    target: ClamdTarget,
}

impl ClamdScanner {
    /// A scanner for `target`. Nothing is connected until the first scan.
    #[must_use]
    pub const fn new(target: ClamdTarget) -> Self {
        Self { target }
    }

    async fn scan_stream<S, R>(stream: &mut S, mut body: R) -> std::io::Result<ScanOutcome>
    where
        S: AsyncRead + AsyncWrite + Unpin,
        R: AsyncRead + Unpin,
    {
        stream.write_all(b"zINSTREAM\0").await?;
        let mut buf = vec![0_u8; CHUNK];
        loop {
            let n = body.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            // `n` is at most CHUNK (64 KiB), well inside u32.
            let len = u32::try_from(n).unwrap_or(u32::MAX);
            stream.write_all(&len.to_be_bytes()).await?;
            stream.write_all(&buf[..n]).await?;
        }
        stream.write_all(&0_u32.to_be_bytes()).await?;
        stream.flush().await?;
        let mut reply = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            if stream.read(&mut byte).await? == 0 {
                break;
            }
            if byte[0] == 0 || byte[0] == b'\n' {
                break;
            }
            reply.push(byte[0]);
            if reply.len() > MAX_REPLY {
                return Ok(ScanOutcome::Failed("clamd sent an oversized reply".into()));
            }
        }
        Ok(parse_reply(&String::from_utf8_lossy(&reply)))
    }
}

/// Read clamd's one-line answer.
fn parse_reply(line: &str) -> ScanOutcome {
    let line = line.trim();
    if let Some(rest) = line.strip_suffix(" FOUND") {
        let name = rest.strip_prefix("stream:").unwrap_or(rest).trim();
        return ScanOutcome::Infected(name.to_owned());
    }
    if line == "stream: OK" {
        return ScanOutcome::Clean;
    }
    if line.ends_with("ERROR") {
        return ScanOutcome::Failed(format!("clamd said: {line}"));
    }
    ScanOutcome::Failed("clamd gave an answer this server does not understand".into())
}

impl ScanHook for ClamdScanner {
    async fn scan<R>(&self, _size: u64, body: R) -> ScanOutcome
    where
        R: AsyncRead + Send + Unpin,
    {
        let result = match &self.target {
            ClamdTarget::Tcp(address) => {
                match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
                    Ok(Ok(mut stream)) => Self::scan_stream(&mut stream, body).await,
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(std::io::ErrorKind::TimedOut.into()),
                }
            }
            ClamdTarget::Unix(path) => {
                match tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(path)).await {
                    Ok(Ok(mut stream)) => Self::scan_stream(&mut stream, body).await,
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(std::io::ErrorKind::TimedOut.into()),
                }
            }
        };
        result.unwrap_or_else(|error| {
            ScanOutcome::Failed(format!("could not scan with clamd: {}", error.kind()))
        })
    }
}

/// The scanner the board runs with, chosen by `[files] scan`.
#[derive(Debug, Clone)]
pub enum Scanner {
    /// No scanner.
    None(NoScan),
    /// `ClamAV` over a socket.
    Clamd(ClamdScanner),
}

impl ScanHook for Scanner {
    async fn scan<R>(&self, size: u64, body: R) -> ScanOutcome
    where
        R: AsyncRead + Send + Unpin,
    {
        match self {
            Self::None(scanner) => scanner.scan(size, body).await,
            Self::Clamd(scanner) => scanner.scan(size, body).await,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use tokio::io::duplex;

    /// A fake clamd on a duplex pipe: reads the `INSTREAM` framing exactly as the real one does
    /// and answers with `reply`. Returns the bytes it received.
    async fn fake_clamd<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, reply: &[u8]) -> Vec<u8> {
        let mut command = [0_u8; 10];
        stream.read_exact(&mut command).await.unwrap();
        assert_eq!(&command, b"zINSTREAM\0");
        let mut content = Vec::new();
        loop {
            let mut len = [0_u8; 4];
            stream.read_exact(&mut len).await.unwrap();
            let len = u32::from_be_bytes(len) as usize;
            if len == 0 {
                break;
            }
            let mut chunk = vec![0_u8; len];
            stream.read_exact(&mut chunk).await.unwrap();
            content.extend_from_slice(&chunk);
        }
        stream.write_all(reply).await.unwrap();
        content
    }

    async fn run(content: Vec<u8>, reply: &'static [u8]) -> (ScanOutcome, Vec<u8>) {
        let (mut client, server) = duplex(1 << 20);
        let fake = tokio::spawn(fake_clamd(server, reply));
        let outcome = ClamdScanner::scan_stream(&mut client, content.as_slice())
            .await
            .unwrap();
        (outcome, fake.await.unwrap())
    }

    #[tokio::test]
    async fn the_content_is_framed_and_the_replies_are_read() {
        let big: Vec<u8> = (0..200_000_u32).map(|n| (n % 251) as u8).collect();
        let (outcome, seen) = run(big.clone(), b"stream: OK\0").await;
        assert_eq!(outcome, ScanOutcome::Clean);
        assert_eq!(seen, big, "every chunk arrived, in order, and was framed");
        let (outcome, _) = run(Vec::new(), b"stream: OK\n").await;
        assert_eq!(
            outcome,
            ScanOutcome::Clean,
            "an empty file is one terminator"
        );
        let (outcome, seen) = run(b"X5O!".to_vec(), b"stream: Eicar-Test-Signature FOUND\0").await;
        assert_eq!(
            outcome,
            ScanOutcome::Infected("Eicar-Test-Signature".into())
        );
        assert_eq!(seen, b"X5O!");
    }

    #[tokio::test]
    async fn errors_and_nonsense_are_failures_never_clean() {
        for reply in [
            &b"INSTREAM size limit exceeded. ERROR\0"[..],
            b"UNKNOWN COMMAND\0",
            b"\0",
            b"stream: OK but not really\0",
            b"stream: OKAY\0",
        ] {
            let (outcome, _) = run(b"data".to_vec(), reply.to_vec().leak()).await;
            assert!(
                matches!(outcome, ScanOutcome::Failed(_)),
                "{reply:?} -> {outcome:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_daemon_that_vanishes_or_talks_forever_is_a_failure() {
        // Closes without a reply.
        let (mut client, server) = duplex(1 << 20);
        drop(server);
        let result = ClamdScanner::scan_stream(&mut client, b"x".as_slice()).await;
        assert!(result.is_err() || matches!(result, Ok(ScanOutcome::Failed(_))));
        // An endless reply line.
        let (mut client, mut server) = duplex(1 << 20);
        tokio::spawn(async move {
            let mut sink = vec![0_u8; 64];
            let _ = server.read(&mut sink).await;
            let _ = server.write_all(&vec![b'A'; 2 * MAX_REPLY]).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let outcome = ClamdScanner::scan_stream(&mut client, b"x".as_slice())
            .await
            .unwrap_or_else(|e| ScanOutcome::Failed(e.to_string()));
        assert!(matches!(outcome, ScanOutcome::Failed(_)));
    }

    #[tokio::test]
    async fn over_a_real_tcp_socket_and_a_dead_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            fake_clamd(stream, b"stream: Win.Test.EICAR_HDB-1 FOUND\0").await
        });
        let scanner = ClamdScanner::new(ClamdTarget::Tcp(address));
        let outcome = scanner.scan(4, b"EICA".as_slice()).await;
        assert_eq!(
            outcome,
            ScanOutcome::Infected("Win.Test.EICAR_HDB-1".into())
        );
        assert_eq!(server.await.unwrap(), b"EICA");
        // Nothing listening: a failure, not a clean file, and not a hang.
        let dead = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().to_string()
        };
        let outcome = ClamdScanner::new(ClamdTarget::Tcp(dead))
            .scan(1, b"x".as_slice())
            .await;
        assert!(matches!(outcome, ScanOutcome::Failed(_)), "{outcome:?}");
    }

    #[tokio::test]
    async fn over_a_unix_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clamd.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            fake_clamd(stream, b"stream: OK\0").await
        });
        let outcome = ClamdScanner::new(ClamdTarget::Unix(path))
            .scan(2, b"hi".as_slice())
            .await;
        assert_eq!(outcome, ScanOutcome::Clean);
        assert_eq!(server.await.unwrap(), b"hi");
    }
}
