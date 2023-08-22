use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use config::Config;
use mime2ext::mime2ext;
use rand::Rng;
use rand::distributions::Alphanumeric;
use tokio::fs::File;
use tokio::net::{ TcpListener, TcpStream };
use tokio::io::{ BufReader, AsyncBufReadExt, AsyncWriteExt, AsyncReadExt };

#[tokio::main]
async fn main() {
    let config = Config::builder()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("APP"))
        .build()
        .unwrap();

    let server_port = &config.get_int("server-port").expect("server-port not set");
    let server = TcpListener::bind(format!("0.0.0.0:{}", server_port)).await.unwrap();
    
    loop {
        let (stream, _socket_addr) = server.accept().await.unwrap();

        let config = config.clone();

        tokio::spawn(async move {
            handle_connection(stream, config).await;
        });
    }
}

async fn handle_connection(mut stream: TcpStream, config: Config) {
    let mut buf_reader = BufReader::new(&mut stream);

    let mut inital = vec![];
    buf_reader.read_until(b'\n', &mut inital).await.unwrap();

    let inital = String::from_utf8(inital).unwrap();
    let (method, right) = inital.split_once(" ").unwrap();
    let (path, _http_type) = right.rsplit_once(" ").unwrap();

    if method != "PUT" {
        let response = format_response(b"que?".to_vec(), 405);
        stream.write(&response).await.unwrap();
        return;
    }

    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = vec![];
        buf_reader.read_until(b'\n', &mut line).await.unwrap();

        line.pop();
        line.pop();
        
        if line.len() == 0 {
            break;
        }

        let line = String::from_utf8(line).unwrap();

        //println!("{}", line);

        let (key, value) = line.split_once(": ").unwrap();
        headers.insert(key.to_lowercase().to_owned(), value.to_owned());
    }

    let secret_key = &config.get_string("secret-key").expect("secret-key not set");

    match headers.get("secret-key") {
        Some(key) => {
            if key != secret_key {
                return;
            }
        },
        None => return,
    }

    let content_length = headers.get("content-length");
    if let None = content_length {
        let response = format_response(b"no content-length header".to_vec(), 411);
        stream.write(&response).await.unwrap();
        return;
    }
    let mut content_length = usize::from_str_radix(content_length.unwrap(), 10).unwrap();

    let mut filename: String = match headers.get("file") {
        Some(name) => {
            let file = name.rsplit_once(".");
            if file.is_some() {
                file.unwrap().0.to_owned()
            } else {
                name.to_owned()
            }
        },
        None => "unknown".to_owned()
    };

    let mime = match headers.get("content-type") {
        Some(mime) => {
            match mime2ext(mime) {
                Some(mime_type) => mime_type,
                None => "bin",
            }
        },
        None => "bin",
    };

    println!("-- NEW UPLOAD --");
    println!("Size: {} bytes", content_length);
    println!("Input File: {}.{}", filename, mime);
    println!("Name: {}", filename);
    println!("Type: {}", mime);
    println!("Path: {}", path);

    let save_path = match path {
        "/image" => {
            config.get_string("image-path").expect("image-path not set")
        },
        "/file" => {
            config.get_string("file-path").expect("file-path not set")
        },
        _ => {
            config.get_string("other-path").expect("other-path not set")
        }
    };

    let path: PathBuf;
    let mut randomness: String;
    loop {
        randomness = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect::<String>();

        let try_path = Path::new(&format!("{}/{}-{}.{}", save_path, randomness, filename, mime)).to_owned();
        if !try_path.exists() {
            path = try_path;
            filename = format!("{}-{}", randomness, filename);
            break;
        }
    }

    let buffer_size = config.get_int("buffer-size").expect("buffer-size not set") as usize;

    let mut file = File::create(path).await.unwrap();
    while content_length > 0 {
        let mut buf: Vec<u8> = vec![0; buffer_size];
        let length = buf_reader.read(&mut buf).await.unwrap();

        if length == 0 { break }

        file.write(&buf[0..length]).await.unwrap();

        content_length -= length;
    }

    let response = format_response(format!("{}.{}", filename, mime).as_bytes().to_vec(), 200);
    stream.write(&response).await.unwrap();

    println!("done");
    println!("");
}

fn format_response(data: Vec<u8>, status_code: u16) -> Vec<u8> {
    let mut buffer = vec![];

    write!(&mut buffer, "HTTP/1.1 {}\r\n", status_code).unwrap();
    write!(&mut buffer, "Content-Length: {}\r\n", data.len()).unwrap();
    write!(&mut buffer, "\r\n").unwrap();
    buffer.extend(data);

    return buffer;
}