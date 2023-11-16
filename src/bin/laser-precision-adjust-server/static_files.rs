use std::collections::HashMap;

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
};

use maplit::hashmap;

use mime_guess::mime;
use typescript_converter_macro::include_ts_relative;

use crate::into_body::IntoBody;

lazy_static::lazy_static! {
    // js with map
    static ref JS_DATA: HashMap<&'static str, (&'static str, &'static str, &'static str)> = hashmap! {
        "common" => include_ts_relative!("wwwroot/ts/common.ts"),
        "work" => include_ts_relative!("wwwroot/ts/work.ts"),
        "stat_manual" => include_ts_relative!("wwwroot/ts/stat_manual.ts"),
        "auto" => include_ts_relative!("wwwroot/ts/auto.ts"),
    };

    // css
    static ref CSS_DATA: HashMap<&'static str, &'static str> = hashmap! {
        "site.css" => include_str!("wwwroot/css/site.css"),
        "docs.min.css" => include_str!("wwwroot/css/docs.min.css"),
    };

    // images
    static ref IMAGE_DATA: HashMap<&'static str, (&'static [u8], &'static str)> = hashmap! {
        "favicon.ico" => (include_bytes!("wwwroot/images/favicon.ico").as_ref(), mime::IMAGE.as_ref()),
        "rez.png" => (include_bytes!("wwwroot/images/rez.png").as_ref(), mime::IMAGE_PNG.as_ref()),
    };

    // fonts
    static ref FONTS_DATA: HashMap<&'static str, &'static [u8]> = hashmap! {
        "7Segment.ttf" => include_bytes!("wwwroot/fonts/7Segment.ttf").as_ref(),
    };
}

#[iftree::include_file_tree(
    "
paths = '**'
base_folder = 'src/bin/laser-precision-adjust-server/wwwroot/lib/'
"
)]
pub struct LibraryAsset {
    contents_bytes: &'static [u8],
    relative_path: &'static str,
}

/// Handle static files: js, css, images, etc.
pub(crate) async fn handle_static(Path((path, file)): Path<(String, String)>) -> impl IntoResponse {
    let plan_text_header = [(header::CONTENT_TYPE, mime::TEXT_PLAIN_UTF_8.as_ref())];
    let not_found = StatusCode::NOT_FOUND.into_response();

    match path.as_str() {
        "js" => {
            let (filename, is_map, is_ts) = if file.ends_with(".map") {
                (file.trim_end_matches(".js.map"), true, false)
            } else if file.ends_with(".ts") {
                (file.trim_end_matches(".ts"), false, true)
            } else if file.ends_with(".js") {
                (file.trim_end_matches(".js"), false, false)
            } else {
                return not_found;
            };
            JS_DATA
                .get(&filename)
                .map_or(not_found, |(js, map, ts_code)| {
                    if is_map {
                        (plan_text_header, map.into_body()).into_response()
                    } else if is_ts {
                        (plan_text_header, ts_code.into_body()).into_response()
                    } else {
                        let headers = [(
                            header::CONTENT_TYPE,
                            mime::APPLICATION_JAVASCRIPT_UTF_8.as_ref(),
                        )];
                        let full_js =
                            format!("{}\n//# sourceMappingURL=/static/{}/{}.map", js, path, file);
                        (headers, full_js.into_body()).into_response()
                    }
                })
        }
        "css" => CSS_DATA.get(file.as_str()).map_or(not_found, |css| {
            let headers = [(header::CONTENT_TYPE, mime::TEXT_CSS_UTF_8.as_ref())];
            (headers, css.into_body()).into_response()
        }),
        "images" => IMAGE_DATA.get(file.as_str()).map_or(not_found, |image| {
            let headers = [(header::CONTENT_TYPE, image.1)];
            (headers, image.0.into_body()).into_response()
        }),
        "fonts" => FONTS_DATA.get(file.as_str()).map_or(not_found, |font| {
            let headers = [(header::CONTENT_TYPE, mime::FONT.as_ref())];
            (headers, font.into_body()).into_response()
        }),
        _ => not_found,
    }
}

/// Handle library files: js, css, images, etc.
pub(crate) async fn handle_lib(Path(path): Path<String>) -> impl IntoResponse {
    ASSETS
        .iter()
        .find(|asset| asset.relative_path == path.as_str())
        .map_or(StatusCode::NOT_FOUND.into_response(), |asset| {
            let mime_type = mime_guess::from_path(asset.relative_path).first_or_octet_stream();
            let headers = [(header::CONTENT_TYPE, mime_type.as_ref())];

            (headers, asset.contents_bytes.into_body()).into_response()
        })
}
