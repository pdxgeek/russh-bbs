//! The bridge between SFTP write packets and `BlobStore::put`: a bounded queue the writes go
//! into and a reader the put pulls from.
//!
//! A put sees end-of-file only after an explicit [`Chunk::Commit`]. If the queue's sender goes
//! away without one (the client vanished, the connection dropped, the handle was abandoned) the
//! reader fails with `BrokenPipe`, `put` stores nothing, and a half-sent file can never be
//! mistaken for a whole one.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};
use tokio::sync::mpsc;

/// Chunks queued between the SFTP loop and the blob writer. Together with the 256 KiB packet
/// cap this bounds an upload's memory to about 1 MiB.
pub(super) const QUEUE: usize = 4;

/// What travels on the queue.
pub(super) enum Chunk {
    /// Bytes of the file, in order.
    Data(Vec<u8>),
    /// The client closed the handle: the file is complete.
    Commit,
}

/// The read side the blob store pulls from.
pub(super) struct ChunkReader {
    rx: mpsc::Receiver<Chunk>,
    current: Vec<u8>,
    at: usize,
    committed: bool,
}

impl ChunkReader {
    /// A bounded queue and its reader.
    pub(super) fn channel() -> (mpsc::Sender<Chunk>, Self) {
        let (tx, rx) = mpsc::channel(QUEUE);
        (
            tx,
            Self {
                rx,
                current: Vec::new(),
                at: 0,
                committed: false,
            },
        )
    }
}

impl AsyncRead for ChunkReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if self.at < self.current.len() {
                let take = buf.remaining().min(self.current.len() - self.at);
                let end = self.at + take;
                buf.put_slice(&self.current[self.at..end]);
                self.at = end;
                return Poll::Ready(Ok(()));
            }
            if self.committed {
                return Poll::Ready(Ok(()));
            }
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(Chunk::Data(data))) => {
                    self.current = data;
                    self.at = 0;
                }
                Poll::Ready(Some(Chunk::Commit)) => self.committed = true,
                Poll::Ready(None) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the upload was abandoned",
                    )));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn a_committed_stream_ends_cleanly_and_an_abandoned_one_fails() {
        let (tx, mut reader) = ChunkReader::channel();
        tx.send(Chunk::Data(b"hello ".to_vec())).await.unwrap();
        tx.send(Chunk::Data(b"world".to_vec())).await.unwrap();
        tx.send(Chunk::Commit).await.unwrap();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"hello world");

        let (tx, mut reader) = ChunkReader::channel();
        tx.send(Chunk::Data(b"half".to_vec())).await.unwrap();
        drop(tx);
        let mut out = Vec::new();
        let error = reader.read_to_end(&mut out).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
