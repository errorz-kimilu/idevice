// Thanks DebianArch

use crate::http2::{
    self,
    h2::{SettingsFrame, WindowUpdateFrame},
};
use error::XPCError;
use format::{XPCFlag, XPCMessage, XPCObject};
use log::debug;
use tokio::net::{TcpStream, ToSocketAddrs};

pub mod cdtunnel;
pub mod error;
pub mod format;

pub struct XPCConnection {
    inner: http2::Connection,
    root_message_id: u64,
    reply_message_id: u64,
}

impl XPCConnection {
    pub const ROOT_CHANNEL: u32 = http2::Connection::ROOT_CHANNEL;
    pub const REPLY_CHANNEL: u32 = http2::Connection::REPLY_CHANNEL;
    const INIT_STREAM: u32 = http2::Connection::INIT_STREAM;

    pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self, XPCError> {
        Self::new(Box::new(TcpStream::connect(addr).await?)).await
    }

    pub async fn new(stream: crate::IdeviceSocket) -> Result<Self, XPCError> {
        let mut client = http2::Connection::new(stream).await?;
        client
            .send_frame(SettingsFrame::new(
                [
                    (SettingsFrame::MAX_CONCURRENT_STREAMS, 100),
                    (SettingsFrame::INITIAL_WINDOW_SIZE, 1048576),
                ]
                .into_iter()
                .collect(),
                Default::default(),
            ))
            .await?;
        client
            .send_frame(WindowUpdateFrame::new(Self::INIT_STREAM, 983041))
            .await?;
        let mut xpc_client = Self {
            inner: client,
            root_message_id: 1,
            reply_message_id: 1,
        };
        xpc_client
            .send_recv_message(
                Self::ROOT_CHANNEL,
                XPCMessage::new(
                    Some(XPCFlag::AlwaysSet),
                    Some(XPCObject::Dictionary(Default::default())),
                    None,
                ),
            )
            .await?;

        // we are here. we send data to stream_id 3 yet we get data from stream 1 ???
        xpc_client
            .send_recv_message(
                Self::REPLY_CHANNEL,
                XPCMessage::new(
                    Some(XPCFlag::InitHandshake | XPCFlag::AlwaysSet),
                    None,
                    None,
                ),
            )
            .await?;

        xpc_client
            .send_recv_message(
                Self::ROOT_CHANNEL,
                XPCMessage::new(Some(XPCFlag::Custom(0x201)), None, None),
            )
            .await?;

        Ok(xpc_client)
    }

    pub async fn send_recv_message(
        &mut self,
        stream_id: u32,
        message: XPCMessage,
    ) -> Result<XPCMessage, XPCError> {
        self.send_message(stream_id, message).await?;
        self.read_message(stream_id).await
    }

    pub async fn send_message(
        &mut self,
        stream_id: u32,
        message: XPCMessage,
    ) -> Result<(), XPCError> {
        self.inner
            .write_streamid(stream_id, message.encode(self.root_message_id)?)
            .await?;
        Ok(())
    }

    pub async fn read_message(&mut self, stream_id: u32) -> Result<XPCMessage, XPCError> {
        let mut buf = self.inner.read_streamid(stream_id).await?;
        loop {
            match XPCMessage::decode(&buf) {
                Ok(decoded) => {
                    debug!("Decoded message: {:?}", decoded);
                    match stream_id {
                        1 => self.root_message_id += 1,
                        3 => self.reply_message_id += 1,
                        _ => {}
                    }
                    return Ok(decoded);
                }
                Err(err) => {
                    log::error!("Error decoding message: {:?}", err);
                    buf.extend_from_slice(&self.inner.read_streamid(stream_id).await?);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_works() {
        let mut client = XPCConnection::new(Box::new(
            TcpStream::connect(("fdca:2653:ece9::1", 64497))
                .await
                .unwrap(),
        ))
        .await
        .unwrap();

        let data = client
            .read_message(http2::Connection::ROOT_CHANNEL)
            .await
            .unwrap();
        println!("{:#?}", data);
    }
}
