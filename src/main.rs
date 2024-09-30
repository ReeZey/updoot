use std::fs;

use byte_unit::Byte;
use config::{Config, File, FileFormat};
use mws::{html::Status, utils::{format_response, format_response_with_body}, WebServer};
use once_cell::sync::Lazy;
use tokio::{io::AsyncWriteExt, sync::Mutex};

static CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| {
    Mutex::new(
        Config::builder()
            .add_source(File::new("config.toml", FileFormat::Toml))
            .build().unwrap()
    )
});

#[tokio::main]
async fn main() {
    let config = CONFIG.lock().await;
    let port = config.get_int("port").unwrap() as u16;
    drop(config);

    let server = WebServer::new(true);
    server.listen("0.0.0.0", port, |mut request| async move {
        let config = CONFIG.lock().await;
        let server_key = config.get_string("secret_key").expect("secret_key not found");
        let types = config.get_table("type").expect("expected atleast one path");
        drop(config);

        if request.method != "PUT" {
            request.stream.write_all(&format_response(Status::MethodNotAllowed)).await.unwrap();
            return;
        }

        let file_type = match request.headers.get("type") {
            Some(file) => file,
            None => {
                request.stream.write_all(&format_response(Status::BadRequest)).await.unwrap();
                return;
            }
        };

        let options = types.get(file_type).unwrap().clone().into_table().unwrap();
        let upload_limit = Byte::parse_str(options.get("limit").unwrap().clone().into_string().unwrap(), true).unwrap().as_u64() as usize;

        if request.body.len() > upload_limit {
            request.stream.write_all(&format_response(Status::PayloadTooLarge)).await.unwrap();
            return;
        }

        match request.headers.get("secret-key") {
            Some(key) => {
                if *key != server_key {
                    request.stream.write_all(&format_response(Status::Unauthorized)).await.unwrap();
                    return;
                }
            },
            None => {
                request.stream.write_all(&format_response(Status::Unauthorized)).await.unwrap();
                return;
            }
        }

        let file_name = match request.headers.get("file") {
            Some(file) => file,
            None => {
                request.stream.write_all(&format_response(Status::BadRequest)).await.unwrap();
                return;
            }
        };

        println!("{} just uploaded {} with size of {:?}", request.get_real_ip(None), file_name, request.body.len());
        fs::write(format!("{}/{}", options.get("path").unwrap().clone().into_string().unwrap(), file_name), request.body).unwrap();

        let text = urlencoding::encode(file_name).to_string();
        request.stream.write_all(&format_response_with_body(Status::OK, text)).await.unwrap();
    }).await;
}
