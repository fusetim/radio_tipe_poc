use crate::LoRaDestination;
use smol::prelude::{AsyncBufRead, AsyncRead, AsyncWrite};
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct LoRaSocket<'a> {
    destination: LoRaDestination<'a>,
}

impl<'a> LoRaSocket<'a> {}

/*impl AsyncRead for LoRaSocket {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize>> {

    }
}*/
