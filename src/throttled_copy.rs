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

#[derive(Debug)]
enum State {
    WaitForData,
    Delaying,
    DelayDone,
}

pub async fn copy<R, W>(reader: &mut R, writer: &mut W) -> io::Result<u64>
where
    R: Read + Unpin + ?Sized,
    W: Write + Unpin + ?Sized,
{
    pub struct CopyFuture<'a, R, W: ?Sized> {
        reader: R,
        writer: &'a mut W,
        amt: u64,
        state: State,
        buffer: Vec<u8>,
        delay: Option<Delay>,
    }

    impl<R, W: Unpin + ?Sized> CopyFuture<'_, R, W> {
        fn project(
            self: Pin<&mut Self>,
        ) -> (
            Pin<&mut R>,
            Pin<&mut W>,
            &mut u64,
            &mut Vec<u8>,
            &mut State,
            &mut Option<Delay>,
        ) {
            unsafe {
                let this = self.get_unchecked_mut();
                (
                    Pin::new_unchecked(&mut this.reader),
                    Pin::new(&mut *this.writer),
                    &mut this.amt,
                    &mut this.buffer,
                    &mut this.state,
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
            let (mut reader, mut writer, amt, buffer, state, delay) = self.project();
            match (state) {
                State::WaitForData => {
                    let data = futures_core::ready!(reader.as_mut().poll_fill_buf(cx))?;
                    if data.is_empty() {
                        futures_core::ready!(writer.as_mut().poll_flush(cx))?;
                        *amt = 0;
                        return Poll::Ready(Ok(*amt));
                    }

                    buffer.extend(data);
                    cx.waker().wake_by_ref();
                    *state = State::Delaying;
                    return Poll::Pending;
                } // end WaitForData

                State::Delaying => {
                    let mut sdelay =
                        delay.get_or_insert_with(|| Delay::new(Duration::from_millis(DELAY)));
                    let pdelay = Pin::new(&mut sdelay);
                    let res = match pdelay.poll(cx) {
                        Poll::Ready(()) => {
                            cx.waker().wake_by_ref();
                            *state = State::DelayDone;
                            return Poll::Pending;
                        }
                        Poll::Pending => {
                            cx.waker().wake_by_ref();
                            *state = State::Delaying;
                            return Poll::Pending;
                        }
                    };
                } // end State::Delaying

                State::DelayDone => {
                    // writing
                    let i = futures_core::ready!(writer.as_mut().poll_write(cx, buffer))?;
                    if i == 0 {
                        return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                    }
                    *amt += i as u64;
                    reader.as_mut().consume(i);
                    delay.take();
                    buffer.clear();
                    *state = State::WaitForData;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
            } // end match
        } // end poll
    } // end Future

    let future = CopyFuture {
        reader: BufReader::new(reader),
        writer,
        amt: 0,
        state: State::WaitForData,
        buffer: Vec::with_capacity(1024),
        delay: None,
    };
    future.await
}
