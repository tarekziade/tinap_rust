// https://docs.rs/async-std/0.99.9/src/async_std/io/copy.rs.html#44-100

use std::pin::Pin;
use std::thread;
use std::time::Duration;
use std::ptr;

use async_std::future::{ready, Future};
use async_std::io::{self, BufRead, BufReader, Read, Write};
use async_std::task::{sleep, Context, Poll};

// XXX  make those options for copy()
const DELAY: u64 = 1000;

#[derive(Debug)]
enum State {
    WaitForData,
    StartDelay,
    DelayDone,
    BufferEmpty,
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
        buffer: Vec<u8>
    }

    impl<R, W: Unpin + ?Sized> CopyFuture<'_, R, W> {
        fn project(self: Pin<&mut Self>) -> (Pin<&mut R>, Pin<&mut W>, &mut u64, &mut Vec<u8>, &mut State) {
            unsafe {
                println!("{:p}", self);
                let this = self.get_unchecked_mut();
                println!("{:p}", this);

                (
                    Pin::new_unchecked(&mut this.reader),
                    Pin::new(&mut *this.writer),
                    &mut this.amt,
                    &mut this.buffer,
                    &mut this.state
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
            let (mut reader, mut writer, amt, buffer, state) = self.project();
                println!("{:?}", state);
                loop {
                match state {
                    State::WaitForData => {

                        *state = State::StartDelay;
                        let b = futures_core::ready!(reader.as_mut().poll_fill_buf(cx))?;
                        buffer.extend(b);
                        println!("buffer 1");

                        if buffer.is_empty() {
                            println!("buffer EOF");
                            *state = State::BufferEmpty;
                            return Poll::Pending;
                        }

                        println!("buffer 2");

                        *state = State::StartDelay;
                        println!("buffer 3");

                        return Poll::Pending;
                    }

                    State::StartDelay => {
                        println!("StartDelay");
                        // here we want to check how long we've waited and then change the state
                        *state = State::DelayDone;
                        return Poll::Pending;
                    }

                    State::DelayDone => {
                        println!("DelayDone");
                        let i = futures_core::ready!(writer.as_mut().poll_write(cx, &buffer))?;
                        if i == 0 {
                            return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                        }
                        *amt += i as u64;
                        reader.as_mut().consume(i);
                        *state = State::WaitForData;
                        return Poll::Pending;
                    }

                    State::BufferEmpty => {
                        println!("BufferEmpty");
                        futures_core::ready!(writer.as_mut().poll_flush(cx))?;
                        buffer.clear();
                        *amt = 0;
                        return Poll::Ready(Ok(*amt));
                    }
                }
                }
        }
    }

    let future = CopyFuture {
        reader: BufReader::new(reader),
        writer,
        amt: 0,
        state: State::WaitForData,
        buffer: Vec::with_capacity(1024)
    };
    future.await
}
