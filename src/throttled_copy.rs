// https://docs.rs/async-std/0.99.9/src/async_std/io/copy.rs.html#44-100

use std::pin::Pin;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use async_std::future::{ready, Future};
use async_std::io::{self, BufRead, BufReader, Read, Write};
use async_std::task::{sleep, Context, Poll, Waker};
use futures_timer::Delay;

// XXX  make those options for copy()
const DELAY: u64 = 500;

pub async fn copy<R, W>(reader: &mut R, writer: &mut W) -> io::Result<u64>
where
    R: Read + Unpin + ?Sized,
    W: Write + Unpin + ?Sized,
{
    pub struct CopyFuture<'a, R, W: ?Sized> {
        reader: R,
        writer: &'a mut W,
        amt: u64,
        delay: Option<Delay>,
    }

    impl<R, W: Unpin + ?Sized> CopyFuture<'_, R, W> {
        fn project(
            self: Pin<&mut Self>,
        ) -> (Pin<&mut R>, Pin<&mut W>, &mut u64, &mut Option<Delay>) {
            unsafe {
                let this = self.get_unchecked_mut();
                (
                    Pin::new_unchecked(&mut this.reader),
                    Pin::new(&mut *this.writer),
                    &mut this.amt,
                    &mut this.delay,
                )
            }
        }
    }

    impl<R, W> Future for CopyFuture<'_, R, W>
    where
        R: BufRead,
        W: Write + Unpin + ?Sized,
    {
        type Output = io::Result<u64>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let (mut reader, mut writer, amt, delay) = self.project();
            loop {
                // reading the data...
                let data = futures_core::ready!(reader.as_mut().poll_fill_buf(cx))?;
                if data.is_empty() {
                    futures_core::ready!(writer.as_mut().poll_flush(cx))?;
                    *amt = 0;
                    return Poll::Ready(Ok(*amt));
                }

                // throttling...
                /* XXX this blocks. I don't know why yet
                let mut sdelay = Delay::new(Duration::from_millis(DELAY));
                let pdelay = Pin::new(&mut sdelay);
                futures_core::ready!(pdelay.poll(cx));
                */
                // this works
                let mut sdelay =
                    delay.get_or_insert_with(|| Delay::new(Duration::from_millis(DELAY)));
                let pdelay = Pin::new(&mut sdelay);
                futures_core::ready!(pdelay.poll(cx));

                // writing the data...
                let i = futures_core::ready!(writer.as_mut().poll_write(cx, data))?;
                if i == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                *amt += i as u64;
                reader.as_mut().consume(i);
                delay.take();
            } // end loop
        } // end poll
    } // end Future

    let future = CopyFuture {
        reader: BufReader::new(reader),
        writer,
        amt: 0,
        delay: None,
    };
    future.await
}
