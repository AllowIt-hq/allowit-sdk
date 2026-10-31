//! Local HTTP script fixture for vendor transport tests.
pub(crate) mod tests {
    use std::{io::Read, time::Duration};
    pub(crate) struct Config {
        pub origin: String,
    }
    pub(crate) struct Client {
        pub cfg: Config,
    }
    use std::{
        io::{ErrorKind, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread::JoinHandle,
        time::Instant,
    };
    pub(crate) enum Reply {
        Close,
        Status(u16, &'static str),
    }
    pub(crate) struct Seen {
        pub head: String,
        pub body: Vec<u8>,
    }
    pub(crate) struct Server {
        listener: Arc<TcpListener>,
        done: Arc<AtomicBool>,
        handle: JoinHandle<Vec<Seen>>,
    }
    fn read_request(stream: &mut TcpStream) -> Seen {
        let mut head = Vec::new();
        let mut byte = [0];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let length = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|n| n.parse().ok())
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        Seen { head, body }
    }
    /// Serve one scripted reply per connection until the script ends or the client returns.
    pub(crate) fn serve(replies: Vec<Reply>) -> (Client, Server) {
        let listener = Arc::new(TcpListener::bind("127.0.0.1:0").unwrap());
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let accept = listener.clone();
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(20);
            for reply in replies {
                let mut stream = loop {
                    match accept.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            if stop.load(Ordering::SeqCst) || Instant::now() > deadline {
                                return seen;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                seen.push(read_request(&mut stream));
                if let Reply::Status(status, body) = reply {
                    write!(
                        stream,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nLocation: /elsewhere\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                }
            }
            seen
        });
        let cfg = Config { origin };
        (
            Client { cfg },
            Server {
                listener,
                done,
                handle,
            },
        )
    }
    impl Server {
        /// Join the script and prove the client opened no further connection before returning.
        pub(crate) fn finish(self) -> Vec<Seen> {
            self.done.store(true, Ordering::SeqCst);
            let seen = self.handle.join().unwrap();
            match self.listener.accept() {
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                _ => panic!("client opened an unexpected extra connection"),
            }
            seen
        }
    }
}
