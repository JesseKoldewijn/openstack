//! A [`tokio::io::AsyncRead`] adapter that computes a running digest as
//! bytes flow through it.
//!
//! Wrap any `AsyncRead + Unpin` reader with [`HashingReader`] before
//! passing it to I/O operations.  When the read is complete, call
//! [`HashingReader::finalize`] to obtain the computed digest output.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use digest::{Digest, Output};

/// An [`AsyncRead`](tokio::io::AsyncRead) adapter that feeds every byte read
/// through a [`Digest`] accumulator.
///
/// # Example
///
/// ```rust,ignore
/// use md5::Md5;
/// use openstack_service_framework::hashing::HashingReader;
///
/// let mut reader = HashingReader::<Md5>::new(some_async_reader);
/// tokio::io::copy(&mut reader, &mut dest).await?;
/// let digest_bytes = reader.finalize();
/// let hex = hex::encode(digest_bytes);
/// ```
pub struct HashingReader<D: Digest, R> {
    inner: R,
    digest: D,
}

impl<D: Digest, R: tokio::io::AsyncRead + Unpin> HashingReader<D, R> {
    /// Wrap `reader` with a fresh digest accumulator.
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            digest: D::new(),
        }
    }

    /// Consume the reader and return the digest output.
    ///
    /// This should be called after all bytes have been read (i.e. after
    /// `tokio::io::copy` returns).
    pub fn finalize(self) -> Output<D> {
        self.digest.finalize()
    }
}

impl<D: Digest + Unpin, R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead
    for HashingReader<D, R>
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let filled_before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let new_bytes = &buf.filled()[filled_before..];
            if !new_bytes.is_empty() {
                self.digest.update(new_bytes);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt as _;

    use super::*;

    // Use md-5 (RustCrypto) which implements digest::Digest.
    type Md5 = md5::Md5;

    #[tokio::test]
    async fn md5_over_known_bytes() {
        let data = b"hello world";
        // Expected MD5 of "hello world" = 5eb63bbbe01eeed093cb22bb8f5acdc3
        let expected = hex::decode("5eb63bbbe01eeed093cb22bb8f5acdc3").unwrap();

        let cursor = tokio::io::BufReader::new(&data[..]);
        let mut reader = HashingReader::<Md5, _>::new(cursor);
        let mut sink = tokio::io::sink();
        tokio::io::copy(&mut reader, &mut sink).await.unwrap();
        let output = reader.finalize();
        assert_eq!(output.as_slice(), expected.as_slice());
    }

    #[tokio::test]
    async fn empty_input_matches_known_md5() {
        // MD5("") = d41d8cd98f00b204e9800998ecf8427e
        let expected = hex::decode("d41d8cd98f00b204e9800998ecf8427e").unwrap();

        let cursor = tokio::io::BufReader::new(&b""[..]);
        let mut reader = HashingReader::<Md5, _>::new(cursor);
        let mut sink = tokio::io::sink();
        tokio::io::copy(&mut reader, &mut sink).await.unwrap();
        let output = reader.finalize();
        assert_eq!(output.as_slice(), expected.as_slice());
    }

    #[tokio::test]
    async fn digest_accumulates_across_multiple_reads() {
        let data = b"the quick brown fox";
        // Break into small reads via a cursor — we get multiple poll_read calls.
        let cursor = tokio::io::BufReader::with_capacity(4, &data[..]);
        let mut reader = HashingReader::<Md5, _>::new(cursor);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        let output = reader.finalize();

        // Independently compute expected.
        use md5::Digest as _;
        let expected = Md5::digest(data);
        assert_eq!(output.as_slice(), expected.as_slice());
        assert_eq!(&buf, data);
    }
}
