mod throttled_copy;
extern crate graceful;

use std::sync::atomic::{ATOMIC_BOOL_INIT, AtomicBool, Ordering};

use async_macros::join;
use async_std::io;
use async_std::net::{TcpListener, TcpStream};
use async_std::prelude::*;
use async_std::task;
use std::sync::Arc;

use structopt::StructOpt;
use graceful::SignalGuard;

use throttled_copy::copy;

#[derive(StructOpt, Debug)]
#[structopt(name = "Tinap port forwarder")]
struct Opt {
    #[structopt(short, long)]
    debug: bool,

    #[structopt(long, default_value = "localhost")]
    host: String,

    #[structopt(long, default_value = "8080")]
    port: u16,

    #[structopt(long, default_value = "localhost")]
    forward_host: String,

    #[structopt(long, default_value = "8181")]
    forward_port: u16,

    #[structopt(long, default_value = "0")]
    delay: u64,

    #[structopt(long, default_value = "0.0")]
    maxbps: f64,

    /// Verbose mode (-v, -vv, -vvv, etc.)
    #[structopt(short, long, parse(from_occurrences))]
    verbose: u8,
}


static STOP: AtomicBool = ATOMIC_BOOL_INIT;


async fn process(stream: TcpStream, opt: Opt) -> io::Result<u64> {
    println!("Accepted from: {}", stream.peer_addr()?);
    let (reader, writer) = &mut (&stream, &stream);
    let upstream = TcpStream::connect((opt.forward_host.as_str(), opt.forward_port)).await?;
    let (upstream_reader, upstream_writer) = &mut (&upstream, &upstream);
    let up = copy(reader, upstream_writer, opt.delay, opt.maxbps);
    let down = copy(upstream_reader, writer, opt.delay, opt.maxbps);
    let res = join!(up, down).await;
    match res {
        (Ok(up_res), Ok(down_res)) => Ok(up_res + down_res),
        (_, _) => Err(io::Error::new(io::ErrorKind::Other, "Bam")),
    }
}

fn main() -> io::Result<()> {
    let signal_guard = SignalGuard::new();
    let opt = Arc::new(Opt::from_args());

    signal_guard.at_exit(move |sig| {
        println!("Signal {} received.", sig);
        STOP.store(true, Ordering::Release);
        // XXX here we want to:
        // 1/ stop listening to incoming connections
        // 2/ gracefully stop any running copy()
        // 3/ end the task
    });

    task::block_on(async move {
        let listener = TcpListener::bind((opt.host.as_str(), opt.port)).await?;

        if opt.verbose > 0 {
            println!("Listening on {}", listener.local_addr()?);
        }

        let mut incoming = listener.incoming();

        while let Some(stream) = incoming.next().await {
            let stream = stream?;
            let opt = Arc::clone(&opt);
            task::spawn(async move {
                process(stream, Arc::try_unwrap(opt).unwrap()).await;
            });
        }

        Ok(())
    })
}
