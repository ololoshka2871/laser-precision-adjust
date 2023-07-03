use bytes::{BufMut, BytesMut};

use tokio_util::codec::{Decoder, Encoder};

use crate::gcode_ctrl::GCodeCtrl;

pub enum CmdResp {
    Ok,
    Err,
}

pub(crate) struct LineCodec;

impl Decoder for LineCodec {
    type Item = CmdResp;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(None)
    }

    fn framed<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Sized>(
        self,
        io: T,
    ) -> tokio_util::codec::Framed<T, Self>
    where
        Self: Sized,
    {
        tokio_util::codec::Framed::new(io, self)
    }
}

impl Encoder<GCodeCtrl> for LineCodec {
    type Error = std::io::Error;

    fn encode(&mut self, req_type: GCodeCtrl, buf: &mut BytesMut) -> Result<(), Self::Error> {
        Ok(())
    }
}