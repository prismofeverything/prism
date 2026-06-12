//! Minimal, dependency-free HTTP/1.1 server plumbing — the mesh transport's
//! shared substrate. A [`TcpListener`] accept loop + request parsing + a response
//! writer, parameterized by a per-request [`Handler`].
//!
//! **One HTTP-serve door.** Both protocol servers run on this:
//! [`super::rest_server`] (the process-RPC wire) and [`super::registry`] (the
//! package store) are two *handlers* over the SAME plumbing. So the package
//! registry literally **reuses** the mesh transport rather than cloning a second
//! hand-rolled server — the "reuse not clone" mandate, made structural.
//!
//! The HTTP layer is hand-rolled (the same dependency-free approach as the
//! `rest_protocol` test mock) because the wire is a handful of JSON routes. One
//! thread per connection, so handlers run concurrently (the rest-concurrency
//! property, #21): a slow `update`/`fetch` never blocks another connection.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A parsed HTTP request — exactly what the protocol handlers need (method, path,
/// body). Headers beyond `Content-Length` are not surfaced (none is needed yet;
/// add a field here when one is, so the parsing stays in one place).
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// Turns a [`Request`] into `(status line, body)`. The status is a `&'static str`
/// literal (`"200 OK"`, `"404 Not Found"`, `"409 Conflict"`, …); the body is the
/// payload. A blanket impl makes any `Fn(&Request) -> (&'static str, String)`
/// a handler, so callers pass a closure capturing their own state (a `Core`, a
/// package store) — no boilerplate trait object on their side.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: &Request) -> (&'static str, String);

    /// The `Content-Type` for a response to `path`. Default `application/json` (the
    /// rest / registry JSON APIs, and the closure handlers). The web boundary
    /// overrides it — `text/html` for the shell, `image/svg+xml` for the rendered
    /// state — so a browser renders the page instead of trying to parse it as JSON.
    fn content_type(&self, _path: &str) -> &'static str {
        "application/json"
    }
}

impl<F> Handler for F
where
    F: Fn(&Request) -> (&'static str, String) + Send + Sync + 'static,
{
    fn handle(&self, req: &Request) -> (&'static str, String) {
        self(req)
    }
}

/// An HTTP server bound to an address, serving a [`Handler`] on a background
/// thread. [`start`](HttpServer::start) binds (port `0` ⇒ an OS-chosen free port,
/// read back via [`port`](HttpServer::port)); drop shuts down and joins.
pub struct HttpServer {
    port: u16,
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HttpServer {
    /// Bind `addr` and serve `handler` until dropped. Each connection is handled on
    /// its own thread, so handlers run concurrently.
    pub fn start<H: Handler>(addr: impl ToSocketAddrs, handler: H) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);

        let thread = {
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || {
                while !shutdown.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let handler = Arc::clone(&handler);
                            thread::spawn(move || {
                                if let Err(e) = serve_conn(stream, handler.as_ref()) {
                                    eprintln!("http server: {e}");
                                }
                            });
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };

        Ok(Self {
            port,
            shutdown,
            thread: Some(thread),
        })
    }

    /// The bound port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:{port}` — the base URL clients address.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Read one request off `stream`, run `handler`, write the response. Parses the
/// request line + `Content-Length` (the only header any handler needs) and reads
/// exactly that many body bytes.
fn serve_conn(mut stream: TcpStream, handler: &dyn Handler) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    // Request line: METHOD PATH HTTP/1.1
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    // Headers — we only need Content-Length.
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 || header == "\r\n" {
            break;
        }
        if let Some(rest) = header.to_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let req = Request {
        method,
        path,
        body,
    };
    let (status, payload) = handler.handle(&req);
    let content_type = handler.content_type(&req.path);

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}
