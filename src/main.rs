use std::{fs::{self, File}, io::{self, Write}, panic, path::PathBuf};
use byte_unit::{Byte, UnitType};
use config::{Config, FileFormat};
use mws::{html::Status, utils::{format_response, format_response_with_body}, WebServer};
use once_cell::sync::Lazy;
use rand::{distributions::Alphanumeric, Rng};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::Mutex};

static CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| {
    Mutex::new(
        Config::builder()
            .add_source(config::File::new("config.toml", FileFormat::Toml))
            .build().unwrap()
    )
});

const UPDOOT_LOG: &str = "[UPDOOT]";

#[tokio::main]
async fn main() {
    let config = CONFIG.lock().await;
    let port = config.get_int("port").expect("port variable is missing") as u16;
    let verbose = config.get_bool("verbose").expect("verbose variable is missing");
    
    for (key, value) in config.get_table("type").expect("expected atleast one path") {
        let options = value.clone().into_table().unwrap();
        options.get("limit").expect(&format!("{:?} type is missing limit variable", key));
        
        let path = options.get("path").expect(&format!("{:?} type is missing path variable", key)).to_string();
        if !PathBuf::from(&path).exists() {
            fs::create_dir_all(&path).unwrap();
            println!("created folders for {:?}", path);
        }
    }

    drop(config);

    let server = WebServer::new("0.0.0.0", port, verbose);
    server.listen(|mut request| async move {
        let config = CONFIG.lock().await;
        let mut server_key = config.get_string("secret_key").unwrap_or("".to_string());
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

        let file_name = match request.headers.get("file") {
            Some(file) => file,
            None => {
                request.stream.write_all(&format_response(Status::BadRequest)).await.unwrap();
                return;
            }
        };

        let key = match request.headers.get("secret-key") {
            Some(key) => key.clone(),
            None => {
                "".to_string()
            }
        };

        let options = match types.get(file_type) {
            Some(value) => {
                value.clone().into_table().unwrap()
            },
            None => {
                request.stream.write_all(&format_response(Status::BadRequest)).await.unwrap();
                return;
            }
        };

        if let Some(key) = options.get("secret_key") {
            server_key = key.to_string();
        }

        if *key != server_key {
            request.stream.write_all(&format_response(Status::Unauthorized)).await.unwrap();
            return;
        }

        let upload_limit = Byte::parse_str(options.get("limit").unwrap().clone().into_string().unwrap(), true).unwrap().as_u64() as usize;
        let total_size = request.get_header("content-length").unwrap_or_default().parse::<usize>().unwrap();

        if total_size == 0 {
            request.stream.write_all(&format_response(Status::LengthRequired)).await.unwrap();
            return;
        }
        
        if total_size > upload_limit {
            request.stream.write_all(&format_response(Status::PayloadTooLarge)).await.unwrap();
            return;
        }

        let folder_path = options.get("path").unwrap().clone().into_string().unwrap();
        let file = format!("{}/{}", folder_path, file_name);
        let mut file_path = PathBuf::from(file);
        if file_path.exists() {
            let rand_string: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(8)
                .map(char::from)
                .collect();

            let file_name = file_path.file_stem().unwrap_or_default().to_str().unwrap();
            let ext = file_path.extension().unwrap_or_default().to_str().unwrap();

            let file = format!("{}/{}_{}.{}", folder_path, file_name, rand_string, ext);
            file_path = PathBuf::from(file)
        }
        let mut file = File::create(file_path).unwrap();

        let mut bytes_read = 0;
        while bytes_read < total_size {
            let mut buffer = vec![0; 1024];
            let bytes = request.stream.read(&mut buffer).await.unwrap();
            
            file.write_all(&buffer[..bytes]).unwrap();
            bytes_read += bytes;
        }

        let convenient_byte = Byte::from_u64(total_size as u64).get_appropriate_unit(UnitType::Decimal);
        
        println!("{UPDOOT_LOG} {:?} just uploaded {:?} [{:.2}]", request.get_real_ip(None), file_name, convenient_byte);

        let text = urlencoding::encode(file_name).to_string();
        request.stream.write_all(&format_response_with_body(Status::OK, text)).await.unwrap();
    }).await;
}
