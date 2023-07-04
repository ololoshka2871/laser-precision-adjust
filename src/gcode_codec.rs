use std::fmt::Write;

use bytes::{Buf, BytesMut};

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
        let mut line = String::new();
        let mut found = false;
        while src.has_remaining() {
            let b = src.get_u8();
            if b == b'\n' {
                found = true;
                break;
            }
            line.push(b as char);
        }
        if found {
            src.clear();
            match line == "ok" {
                true => Ok(Some(CmdResp::Ok)),
                false => Ok(Some(CmdResp::Err)),
            }
        } else {
            log::trace!("No newline found in buffer: [{}]", line);
            Ok(None)
        }
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
        buf.write_fmt(format_args!("{}", req_type)).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to write to buffer: {}", e),
            )
        })
    }
}
