mod throttled_copy;
use std::time::Duration;

use async_macros::join;
use async_std::io;
use async_std::io::Error;
use async_std::net::{TcpListener, TcpStream};
use async_std::prelude::*;
use async_std::task;

use async_std::io::{Read, Write};
use futures::Poll;
use futures_io::{AsyncRead, AsyncWrite};
use std::pin::Pin;
use throttled_copy::copy;

async fn process(stream: TcpStream) -> io::Result<u64> {
    println!("Accepted from: {}", stream.peer_addr()?);
    let (reader, writer) = &mut (&stream, &stream);
    let upstream = TcpStream::connect("127.0.0.1:8081").await?;
    let (upstream_reader, upstream_writer) = &mut (&upstream, &upstream);
    let up = copy(reader, upstream_writer);
    let down = copy(upstream_reader, writer);
    let res = join!(up, down).await;
    match res {
        (Ok(up_res), Ok(down_res)) => Ok(up_res + down_res),
        (_, _) => Err(io::Error::new(io::ErrorKind::Other, "Bam")),
    }
}

fn main() -> io::Result<()> {
    task::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:8080").await?;
        println!("Listening on {}", listener.local_addr()?);
        let mut incoming = listener.incoming();

        while let Some(stream) = incoming.next().await {
            let stream = stream?;
            task::spawn(async {
                process(stream).await;
            });
        }
        Ok(())
    })
}
