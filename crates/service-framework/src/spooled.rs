//! A request body that spools to a temporary file when it exceeds a
//! configurable threshold, keeping small payloads in memory.

use std::io::{self, Read, Seek, Write};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use tempfile::SpooledTempFile;

/// A request body backed by a [`SpooledTempFile`].
///
/// Payloads smaller than `threshold` bytes stay in memory.  Once the
/// threshold is exceeded the data is transparently spilled to a
/// temporary file on disk.
pub struct SpooledBody {
    inner: SpooledTempFile,
    len: usize,
}

impl std::fmt::Debug for SpooledBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpooledBody")
            .field("len", &self.len)
            .field("is_rolled", &self.is_rolled())
            .finish()
    }
}

impl SpooledBody {
    /// Create a new, empty `SpooledBody` with the given in-memory
    /// threshold in bytes.
    pub fn new(threshold: usize) -> Self {
        Self {
            inner: SpooledTempFile::new(threshold),
            len: 0,
        }
    }

    /// Create a `SpooledBody` pre-populated with the given bytes.
    ///
    /// If the data exceeds `threshold` it will be written to a temp
    /// file immediately.
    pub fn from_bytes(bytes: Bytes, threshold: usize) -> io::Result<Self> {
        let mut body = Self::new(threshold);
        body.write_chunk(&bytes)?;
        body.rewind()?;
        Ok(body)
    }

    /// Append a chunk of data.
    pub fn write_chunk(&mut self, chunk: &[u8]) -> io::Result<()> {
        self.inner.write_all(chunk)?;
        self.len += chunk.len();
        Ok(())
    }

    /// Consume a `futures_core::Stream` of byte chunks, writing each
    /// chunk to the spooled body.
    pub async fn write_from_stream<S>(&mut self, stream: S) -> io::Result<()>
    where
        S: futures_core::Stream<Item = Result<Bytes, io::Error>>,
    {
        tokio::pin!(stream);
        loop {
            let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
            match next {
                Some(Ok(chunk)) => {
                    self.write_chunk(&chunk)?;
                }
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }
        self.rewind()?;
        Ok(())
    }

    /// Materialise the full body as `Bytes`, consuming self.
    ///
    /// If the data is still in memory this is cheap; if it has been
    /// spooled to disk the file is read back.
    pub fn into_bytes(mut self) -> io::Result<Bytes> {
        self.inner.rewind()?;
        let mut buf = Vec::with_capacity(self.len);
        self.inner.read_to_end(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    /// Return a copy of the body as `Bytes` without consuming self.
    ///
    /// The internal cursor is rewound before *and* after the read so
    /// that subsequent reads start from the beginning.
    pub fn to_bytes(&mut self) -> io::Result<Bytes> {
        self.inner.rewind()?;
        let mut buf = Vec::with_capacity(self.len);
        self.inner.read_to_end(&mut buf)?;
        self.inner.rewind()?;
        Ok(Bytes::from(buf))
    }

    /// Rewind the internal cursor to the beginning.
    pub fn rewind(&mut self) -> io::Result<()> {
        self.inner.rewind()?;
        Ok(())
    }

    /// Convert into an [`AsyncRead`](tokio::io::AsyncRead) adapter.
    pub fn into_reader(self) -> SpooledBodyReader {
        SpooledBodyReader { inner: self }
    }

    /// Total number of bytes written.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no bytes have been written.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the data has been spilled to disk.
    pub fn is_rolled(&self) -> bool {
        self.inner.is_rolled()
    }
}

/// An [`AsyncRead`](tokio::io::AsyncRead) wrapper around [`SpooledBody`].
///
/// Because `SpooledTempFile` implements synchronous `Read`, reads are
/// performed on the current thread (they hit either a `Vec<u8>` or a
/// temporary file). For production use with very large bodies consider
/// wrapping in `tokio::task::spawn_blocking` — but for the typical
/// LocalStack workload the synchronous path is fine.
pub struct SpooledBodyReader {
    inner: SpooledBody,
}

impl tokio::io::AsyncRead for SpooledBodyReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let unfilled = buf.initialize_unfilled();
        match self.inner.inner.read(unfilled) {
            Ok(n) => {
                buf.advance(n);
                Poll::Ready(Ok(()))
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_payload_stays_in_memory() {
        let mut body = SpooledBody::new(1024);
        body.write_chunk(b"hello").unwrap();
        assert!(!body.is_rolled());
        assert_eq!(body.len(), 5);
        let bytes = body.into_bytes().unwrap();
        assert_eq!(&bytes[..], b"hello");
    }

    #[test]
    fn large_payload_spills_to_disk() {
        let mut body = SpooledBody::new(8);
        body.write_chunk(b"more than eight bytes").unwrap();
        assert!(body.is_rolled());
        assert_eq!(body.len(), 21);
        let bytes = body.into_bytes().unwrap();
        assert_eq!(&bytes[..], b"more than eight bytes");
    }

    #[test]
    fn from_bytes_round_trips() {
        let original = Bytes::from_static(b"round-trip test data");
        let body = SpooledBody::from_bytes(original.clone(), 1024).unwrap();
        let result = body.into_bytes().unwrap();
        assert_eq!(result, original);
    }

    #[test]
    fn to_bytes_does_not_consume() {
        let mut body = SpooledBody::new(1024);
        body.write_chunk(b"keep me").unwrap();
        body.rewind().unwrap();
        let first = body.to_bytes().unwrap();
        let second = body.to_bytes().unwrap();
        assert_eq!(first, second);
        assert_eq!(&first[..], b"keep me");
    }

    #[tokio::test]
    async fn async_read_adapter() {
        use tokio::io::AsyncReadExt;

        let mut body = SpooledBody::new(1024);
        body.write_chunk(b"async read test").unwrap();
        body.rewind().unwrap();
        let mut reader = body.into_reader();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(&buf, b"async read test");
    }

    #[tokio::test]
    async fn write_from_stream_works() {
        use std::pin::Pin;
        use std::task::{Context, Poll};

        /// A simple stream that yields a fixed list of chunks.
        struct ChunkStream {
            chunks: Vec<Result<Bytes, io::Error>>,
            index: usize,
        }

        impl futures_core::Stream for ChunkStream {
            type Item = Result<Bytes, io::Error>;

            fn poll_next(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                if self.index < self.chunks.len() {
                    let idx = self.index;
                    self.index += 1;
                    // We need to take the item out; use a placeholder error then swap.
                    // Actually, since we control the vec, just swap_remove won't work
                    // because order matters. Use Option wrapping instead.
                    // Simpler: reconstruct with drain. But we can't drain a pinned vec
                    // easily. Let's just use mem::replace with a sentinel.
                    let item = std::mem::replace(
                        &mut self.chunks[idx],
                        Ok(Bytes::new()), // placeholder
                    );
                    Poll::Ready(Some(item))
                } else {
                    Poll::Ready(None)
                }
            }
        }

        let stream = ChunkStream {
            chunks: vec![
                Ok(Bytes::from_static(b"chunk1")),
                Ok(Bytes::from_static(b"chunk2")),
                Ok(Bytes::from_static(b"chunk3")),
            ],
            index: 0,
        };

        let mut body = SpooledBody::new(1024);
        body.write_from_stream(stream).await.unwrap();

        assert_eq!(body.len(), 18);
        let bytes = body.into_bytes().unwrap();
        assert_eq!(&bytes[..], b"chunk1chunk2chunk3");
    }

    #[test]
    fn empty_body() {
        let body = SpooledBody::new(1024);
        assert!(body.is_empty());
        assert_eq!(body.len(), 0);
        assert!(!body.is_rolled());
        let bytes = body.into_bytes().unwrap();
        assert!(bytes.is_empty());
    }
}
