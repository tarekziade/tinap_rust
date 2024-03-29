mod throttled_copy;
extern crate graceful;

use async_macros::join;
use async_std::future::select;
use async_std::io;
use async_std::net::{TcpListener, TcpStream};
use async_std::prelude::*;
use async_std::task;
use futures::channel::mpsc;
use std::thread;

use graceful::SignalGuard;
use structopt::StructOpt;

use throttled_copy::copy;

#[derive(StructOpt, Debug)]
#[structopt(name = "Tinap port forwarder")]
#[derive(Clone)]
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

async fn process(stream: TcpStream, opt: &Opt) -> io::Result<u64> {
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
    let (tx, mut rx) = mpsc::channel(10);
    let opt = Opt::from_args();

    let handler = thread::spawn(move || {
        task::block_on(async move {
            let listener = TcpListener::bind((opt.host.as_str(), opt.port)).await?;
            if opt.verbose > 0 {
                println!("Listening on {}", listener.local_addr()?);
            }
            let mut incoming = listener.incoming();

            loop {
                let opt = opt.clone();
                let next = incoming.next();
                let stop = rx.next();
                let res = select!(next, stop).await;
                match res {
                    None => {}
                    Some(stream_res) => match stream_res {
                        Err(_) => {
                            println!("\nBye!");
                            // XXX here, we need to wait for any ongoing process
                            // and eventually kill them after n seconds.
                            break;
                        }
                        Ok(stream) => {
                            task::spawn(async move {
                                process(stream, &opt).await.unwrap();
                            });
                        }
                    },
                }
            }
            Ok::<(), io::Error>(())
        }).unwrap();
    });

    let signal_guard = SignalGuard::new();
    let mut signal_tx = tx.clone();
    signal_guard.at_exit(move |sig| {
        println!("Signal {} received.", sig);
        signal_tx.try_send(Err(io::Error::new(io::ErrorKind::Other, "end!"))).unwrap();
        handler.join().unwrap();
    });

    Ok(())
}
