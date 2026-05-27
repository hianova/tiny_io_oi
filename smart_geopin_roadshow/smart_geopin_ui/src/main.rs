use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

const HTML_CONTENT: &str = include_str!("../static/index.html");
const CSS_CONTENT: &str = include_str!("../static/style.css");
const JS_CONTENT: &str = include_str!("../static/app.js");

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 1024];
    if let Ok(bytes_read) = stream.read(&mut buffer) {
        if bytes_read == 0 {
            return;
        }

        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        let mut lines = request.lines();
        let first_line = lines.next().unwrap_or("");
        
        let mut response = String::new();

        if first_line.starts_with("GET / ") || first_line.starts_with("GET /index.html ") {
            response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                HTML_CONTENT.len(),
                HTML_CONTENT
            );
        } else if first_line.starts_with("GET /style.css ") {
            response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: {}\r\n\r\n{}",
                CSS_CONTENT.len(),
                CSS_CONTENT
            );
        } else if first_line.starts_with("GET /app.js ") {
            response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\n\r\n{}",
                JS_CONTENT.len(),
                JS_CONTENT
            );
        } else if first_line.starts_with("POST /trigger ") {
            // Forward command to ServerGo via RESP
            // redis-cli -x PUT vm:broadcast \x83\x03\x07\x00\x00\x40\x00\x80
            match TcpStream::connect("127.0.0.1:6379") {
                Ok(mut redis_stream) => {
                    let payload = b"\x83\x03\x07\x00\x00\x40\x00\x80";
                    let resp_cmd = format!(
                        "*3\r\n$3\r\nPUT\r\n$12\r\nvm:broadcast\r\n${}\r\n",
                        payload.len()
                    );
                    let mut full_req = resp_cmd.into_bytes();
                    full_req.extend_from_slice(payload);
                    full_req.extend_from_slice(b"\r\n");

                    if redis_stream.write_all(&full_req).is_ok() {
                        response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"success\"}".to_string();
                    } else {
                        response = "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\n\r\n{\"status\":\"error\"}".to_string();
                    }
                }
                Err(e) => {
                    response = format!("HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\n\r\n{{\"status\":\"error\", \"message\":\"{}\"}}", e);
                }
            }
        } else {
            response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found".to_string();
        }

        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    }
}

fn main() {
    let port = 8080;
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).unwrap();
    println!("🚀 Smart Geopin Roadshow UI Server running at http://localhost:{}", port);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| handle_client(stream));
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
            }
        }
    }
}
