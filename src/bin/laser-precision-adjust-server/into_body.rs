use std::io::Cursor;

use axum::body::StreamBody;
use tokio_util::io::ReaderStream;



pub trait IntoBody<T> {
    fn into_body(self) -> StreamBody<ReaderStream<Cursor<T>>>;
}

impl IntoBody<&'static str> for &'static str {
    fn into_body(self) -> StreamBody<ReaderStream<Cursor<&'static str>>> {
        let stream = Cursor::new(self);
        let stream = ReaderStream::new(stream);
        StreamBody::new(stream)
    }
}

impl IntoBody<&'static [u8]> for &'static [u8] {
    fn into_body(self) -> StreamBody<ReaderStream<Cursor<&'static [u8]>>> {
        let stream = Cursor::new(self);
        let stream = ReaderStream::new(stream);
        StreamBody::new(stream)
    }
}

impl IntoBody<String> for String {
    fn into_body(self) -> StreamBody<ReaderStream<Cursor<String>>> {
        let stream = Cursor::new(self);
        let stream = ReaderStream::new(stream);
        StreamBody::new(stream)
    }
}

impl IntoBody<Vec<u8>> for Vec<u8> {
    fn into_body(self) -> StreamBody<ReaderStream<Cursor<Vec<u8>>>> {
        let stream = Cursor::new(self);
        let stream = ReaderStream::new(stream);
        StreamBody::new(stream)
    }
}