use std::{fs::File, io::Write};

use byte_unit::Byte;
use config::{Config, FileFormat};
use mws::{html::Status, utils::{format_response, format_response_with_body}, WebServer};
use once_cell::sync::Lazy;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::Mutex};

static CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| {
    Mutex::new(
        Config::builder()
            .add_source(config::File::new("config.toml", FileFormat::Toml))
            .build().unwrap()
    )
});

#[tokio::main]
async fn main() {
    let config = CONFIG.lock().await;
    let port = config.get_int("port").unwrap() as u16;
    drop(config);

    let server = WebServer::new("0.0.0.0", port, true);
    server.listen(|mut request| async move {
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


        let options = types.get(file_type).unwrap().clone().into_table().unwrap();
        let upload_limit = Byte::parse_str(options.get("limit").unwrap().clone().into_string().unwrap(), true).unwrap().as_u64() as usize;

        let total_size = request.get_header("content-length").unwrap().parse::<usize>().unwrap();

        if total_size > upload_limit {
            request.stream.write_all(&format_response(Status::PayloadTooLarge)).await.unwrap();
            return;
        }

        let file = format!("{}/{}", options.get("path").unwrap().clone().into_string().unwrap(), file_name);
        let mut file = File::create(file).unwrap();

        let mut bytes_read = 0;
        while bytes_read < total_size {
            let mut buffer = vec![0; 1024];
            let bytes = request.stream.read(&mut buffer).await.unwrap();
            
            file.write_all(&buffer[..bytes]).unwrap();
            bytes_read += bytes;
        }

        println!("{} just uploaded {} with size of {:?}", request.get_real_ip(None), file_name, total_size);

        let text = urlencoding::encode(file_name).to_string();
        request.stream.write_all(&format_response_with_body(Status::OK, text)).await.unwrap();
    }).await;
}
