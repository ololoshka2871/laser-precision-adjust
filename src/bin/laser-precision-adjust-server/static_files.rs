use std::collections::HashMap;

use axum::{
    body::StreamBody,
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
};

use maplit::hashmap;

use regex::Regex;
use tokio_util::io::ReaderStream;
use typescript_converter_macro::include_ts;

use crate::AppEngine;

lazy_static::lazy_static! {
    static ref JS_REMOVE_EXT: Regex = Regex::new(r"\..+").unwrap();

    // js with map
    static ref JS_DATA: HashMap<&'static str, (&'static str, &'static str)> = hashmap! {
        "common" => include_ts!("www\\ts\\common.ts"),
    };

    // css
    static ref CSS_DATA: HashMap<&'static str, &'static str> = hashmap! {
        "site.css" => include_str!("www\\css\\site.css"),
    };

    // images
    static ref IMAGE_DATA: HashMap<&'static str, (&'static [u8], &'static str)> = hashmap! {
        "favicon.ico" => (include_bytes!("www\\images\\favicon.ico").as_ref(), "image/x-icon"),
    };
}

/// Handle static files: js, css, images, etc.
pub(crate) async fn handle_static(
    State(_engine): State<AppEngine>,
    Path((path, file)): Path<(String, String)>,
) -> impl IntoResponse {
    let not_found = StatusCode::NOT_FOUND.into_response();

    match path.as_str() {
        "js" => {
            let name = JS_REMOVE_EXT.replace_all(&file, "");
            match JS_DATA.get(name.as_ref()) {
                Some((js, map)) => {
                    if file.ends_with(".map") {
                        map.into_response()
                    } else {
                        js.into_response()
                    }
                }
                None => not_found,
            }
        }
        "css" => CSS_DATA
            .get(file.as_str())
            .map_or(not_found, |css| css.into_response()),
        "images" => IMAGE_DATA.get(file.as_str()).map_or(not_found, |image| {
            let (stream, content_type) = (std::io::Cursor::new(image.0), image.1);
            let body = StreamBody::new(ReaderStream::new(stream));
            let headers = [(header::CONTENT_TYPE, content_type)];

            (headers, body).into_response()
        }),
        _ => not_found,
    }
}
