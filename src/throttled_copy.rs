// https://docs.rs/async-std/0.99.9/src/async_std/io/copy.rs.html#44-100

use std::pin::Pin;
use std::time::Duration;

use async_std::future::Future;
use async_std::io::{self, BufRead, BufReader, Read, Write};
use async_std::task::{Context, Poll};
use conv::*;
use futures_timer::Delay;
use time::precise_time_s;

pub async fn copy<R, W>(
    reader: &mut R,
    writer: &mut W,
    opt_delay: u64,
    opt_maxbps: f64,
) -> io::Result<u64>
where
    R: Read + Unpin + ?Sized,
    W: Write + Unpin + ?Sized,
{
    pub struct CopyFuture<'a, R, W: ?Sized> {
        reader: R,
        writer: &'a mut W,
        amt: u64,
        delay: Option<Delay>,
        last_tick: f64,
        opt_delay: u64,
        opt_maxbps: f64,
    }

    impl<R, W: Unpin + ?Sized> CopyFuture<'_, R, W> {
        fn project(
            self: Pin<&mut Self>,
        ) -> (
            Pin<&mut R>,
            Pin<&mut W>,
            &mut u64,
            &mut Option<Delay>,
            &mut f64,
            u64,
            f64,
        ) {
            unsafe {
                let this = self.get_unchecked_mut();
                (
                    Pin::new_unchecked(&mut this.reader),
                    Pin::new(&mut *this.writer),
                    &mut this.amt,
                    &mut this.delay,
                    &mut this.last_tick,
                    this.opt_delay,
                    this.opt_maxbps,
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
            let (mut reader, mut writer, amt, delay, last_tick, opt_delay, opt_maxbps) =
                self.project();
            loop {
                // reading the data...
                let data = futures_core::ready!(reader.as_mut().poll_fill_buf(cx))?;
                if data.is_empty() {
                    futures_core::ready!(writer.as_mut().poll_flush(cx))?;
                    *amt = 0;
                    return Poll::Ready(Ok(*amt));
                }

                // throttling...
                // the delay is composed of two things:
                // - DELAY: the delay between each round trip
                let mut current_delay = opt_delay;

                // - MAXBPS: the max bits per seconds allowed. we're looking at how much
                // data was sent since last time and if needed, pause more
                let elapsed = precise_time_s() - *last_tick;
                let over_bandwidth = f64::value_from(data.len()).unwrap() - (elapsed * opt_maxbps);
                if over_bandwidth > 0. {
                    let extra_delay = over_bandwidth / opt_maxbps;
                    current_delay = current_delay + extra_delay as u64;
                    *last_tick = precise_time_s()
                }

                //
                /* XXX this blocks. I don't know why yet
                let mut sdelay = Delay::new(Duration::from_millis(DELAY));
                let pdelay = Pin::new(&mut sdelay);
                futures_core::ready!(pdelay.poll(cx));
                */
                // this works
                let mut sdelay =
                    delay.get_or_insert_with(|| Delay::new(Duration::from_millis(current_delay)));
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
        last_tick: precise_time_s(),
        opt_delay: opt_delay,
        opt_maxbps: opt_maxbps,
    };
    future.await
}
