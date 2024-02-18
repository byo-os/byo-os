use interprocess::local_socket::{LocalSocketListener, LocalSocketStream};
use rmp_serde::decode::Error as MsgPackDecodeError;
use rmp_serde::encode::Error as MsgPackEncodeError;
use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};
use std::net::{TcpListener, TcpStream};
use std::{
    io::{Read, Write},
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use url::Url;

pub struct IpcServer<T: Serialize + Deserialize<'static>> {
    listener: IpcListener,
    format: IpcFormat,
    _phantom: PhantomData<T>,
}

impl<T: Serialize + Deserialize<'static>> IpcServer<T> {
    pub fn bind(url: &str) -> Result<Self, IpcError> {
        let url = Url::parse(url)?;

        let (transport, format) = try_parse_schema(&url)?;

        let listener = match transport {
            #[cfg(unix)]
            IpcTransport::Uds => {
                let listener = LocalSocketListener::bind(url.path())?;
                IpcListener::LocalSocket(Arc::new(Mutex::new(listener)))
            }
            #[cfg(any(windows, target_os = "linux"))]
            IpcTransport::Namespace => {
                let listener = LocalSocketListener::bind(format!("@{}", url.path()))?;
                IpcListener::LocalSocket(Arc::new(Mutex::new(listener)))
            }
            IpcTransport::Tcp => {
                let listener = TcpListener::bind(format!(
                    "{}:{}",
                    url.host_str().ok_or(IpcError::MissingHost)?,
                    url.port().ok_or(IpcError::MissingPort)?
                ))?;
                IpcListener::Tcp(Arc::new(Mutex::new(listener)))
            }
        };

        Ok(IpcServer {
            listener,
            format,
            _phantom: PhantomData,
        })
    }

    pub fn accept(&self) -> Result<IpcConnection<T>, IpcError> {
        match &self.listener {
            IpcListener::LocalSocket(listener) => {
                let mut lock = listener.lock().unwrap();
                let stream = &mut *lock;
                Ok(IpcConnection {
                    stream: IpcStream::LocalSocket(Arc::new(Mutex::new(stream.accept()?))),
                    format: self.format,
                    _phantom: PhantomData,
                })
            }
            IpcListener::Tcp(listener) => {
                let mut lock = listener.lock().unwrap();
                let stream = &mut *lock;
                Ok(IpcConnection {
                    stream: IpcStream::Tcp(Arc::new(Mutex::new(stream.accept()?.0))),
                    format: self.format,
                    _phantom: PhantomData,
                })
            }
        }
    }

    pub fn incoming(&self) -> IpcIncoming<T> {
        IpcIncoming {
            listener: &self,
            format: self.format,
            _phantom: PhantomData,
        }
    }
}

fn try_parse_schema(url: &Url) -> Result<(IpcTransport, IpcFormat), IpcError> {
    let (format, transport) = url
        .scheme()
        .split_once("+")
        .ok_or(IpcError::UnsupportedScheme(url.scheme().into()))?;

    let transport: IpcTransport = transport.try_into()?;
    let format: IpcFormat = format.try_into()?;

    Ok((transport, format))
}

pub struct IpcIncoming<'a, T: Serialize + Deserialize<'static>> {
    listener: &'a IpcServer<T>,
    format: IpcFormat,
    _phantom: PhantomData<T>,
}

impl<T: Serialize + Deserialize<'static>> Iterator for IpcIncoming<'_, T> {
    type Item = Result<IpcConnection<T>, IpcError>;

    fn next(&mut self) -> Option<Self::Item> {
        match &self.listener.listener {
            IpcListener::LocalSocket(listener) => {
                let mut lock = listener.lock().unwrap();
                let listener = &mut *lock;
                let stream = match listener.accept() {
                    Ok(stream) => stream,
                    Err(e) => return Some(Err(e.into())),
                };
                Some(Ok(IpcConnection {
                    stream: IpcStream::LocalSocket(Arc::new(Mutex::new(stream))),
                    format: self.format,
                    _phantom: PhantomData,
                }))
            }
            IpcListener::Tcp(listener) => {
                let mut lock = listener.lock().unwrap();
                let listener = &mut *lock;
                let (stream, _) = match listener.accept() {
                    Ok(stream) => stream,
                    Err(e) => return Some(Err(e.into())),
                };
                Some(Ok(IpcConnection {
                    stream: IpcStream::Tcp(Arc::new(Mutex::new(stream))),
                    format: self.format,
                    _phantom: PhantomData,
                }))
            }
        }
    }
}

pub struct IpcConnection<T: Serialize + Deserialize<'static>> {
    stream: IpcStream,
    format: IpcFormat,
    _phantom: PhantomData<T>,
}

impl<T: Serialize + Deserialize<'static>> IpcConnection<T> {
    pub fn open(url: &str) -> Result<Self, IpcError> {
        let url = Url::parse(url)?;

        let (transport, format) = try_parse_schema(&url)?;

        let stream = match transport {
            #[cfg(unix)]
            IpcTransport::Uds => {
                let stream = LocalSocketStream::connect(url.path())?;
                IpcStream::LocalSocket(Arc::new(Mutex::new(stream)))
            }
            #[cfg(any(windows, target_os = "linux"))]
            IpcTransport::Namespace => {
                let stream = LocalSocketStream::connect(format!("@{}", url.path())).unwrap();
                IpcStream::LocalSocket(Arc::new(Mutex::new(stream)))
            }
            IpcTransport::Tcp => {
                let stream = TcpStream::connect(format!(
                    "{}:{}",
                    url.host_str().ok_or(IpcError::MissingHost)?,
                    url.port().ok_or(IpcError::MissingPort)?
                ))
                .unwrap();
                IpcStream::Tcp(Arc::new(Mutex::new(stream)))
            }
        };

        Ok(IpcConnection {
            stream,
            format,
            _phantom: PhantomData,
        })
    }

    pub fn send(&self, message: &T) -> Result<(), IpcError> {
        match &self.stream {
            IpcStream::LocalSocket(stream) => {
                let mut lock = stream.lock().unwrap();
                let write = &mut *lock;
                self.send_inner(write, message)?;
            }
            IpcStream::Tcp(stream) => {
                let mut lock = stream.lock().unwrap();
                let write = &mut *lock;
                self.send_inner(write, message)?;
            }
        }

        Ok(())
    }

    fn send_inner(&self, write: &mut dyn Write, message: &T) -> Result<(), IpcError> {
        match &self.format {
            IpcFormat::MsgPack => {
                let mut serializer = Serializer::new(write);
                message.serialize(&mut serializer)?;
            }
        }
        Ok(())
    }

    pub fn recv(&self) -> Result<T, IpcError> {
        match &self.stream {
            IpcStream::LocalSocket(stream) => {
                let mut lock = stream.lock().unwrap();
                let read = &mut *lock;
                self.recv_inner(read)
            }
            IpcStream::Tcp(stream) => {
                let mut lock = stream.lock().unwrap();
                let read = &mut *lock;
                self.recv_inner(read)
            }
        }
    }

    fn recv_inner(&self, read: &mut dyn Read) -> Result<T, IpcError> {
        match &self.format {
            IpcFormat::MsgPack => {
                let mut deserializer = Deserializer::new(read);
                Ok(T::deserialize(&mut deserializer)?)
            }
        }
    }
}

pub enum IpcListener {
    LocalSocket(Arc<Mutex<LocalSocketListener>>),
    Tcp(Arc<Mutex<TcpListener>>),
}

pub enum IpcStream {
    LocalSocket(Arc<Mutex<LocalSocketStream>>),
    Tcp(Arc<Mutex<TcpStream>>),
}

#[derive(Debug, Clone, Copy)]
pub enum IpcFormat {
    MsgPack,
}

impl TryFrom<&str> for IpcFormat {
    type Error = IpcError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "msgpack" => Ok(IpcFormat::MsgPack),
            _ => Err(IpcError::UnsupportedFormat(value.into())),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IpcTransport {
    #[cfg(unix)]
    Uds,
    #[cfg(any(windows, target_os = "linux"))]
    Namespace,
    Tcp,
}

impl TryFrom<&str> for IpcTransport {
    type Error = IpcError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            #[cfg(unix)]
            "uds" => Ok(IpcTransport::Uds),
            #[cfg(any(windows, target_os = "linux"))]
            "namespace" => Ok(IpcTransport::Namespace),
            "tcp" => Ok(IpcTransport::Tcp),
            _ => Err(IpcError::UnsupportedTransport(value.into())),
        }
    }
}

#[derive(Error, Debug)]
pub enum IpcError {
    #[error("Unsupported scheme: {}. You must use a scheme in the format of 'format+transport', with a plus sign as a separator", .0)]
    UnsupportedScheme(String),
    #[error("Unsupported transport: {}", .0)]
    UnsupportedTransport(String),
    #[error("Unsupported format: {}", .0)]
    UnsupportedFormat(String),
    #[error("Missing host")]
    MissingHost,
    #[error("Missing port")]
    MissingPort,
    #[error("Invalid URL")]
    InvalidUrl(#[from] url::ParseError),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("MsgPack encode error")]
    MsgPackEncode(#[from] MsgPackEncodeError),
    #[error("MsgPack decode error")]
    MsgPackDecode(#[from] MsgPackDecodeError),
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_uds() {
        const FILE: &str = "/tmp/test_byo_ipc.sock";
        const URL: &str = "msgpack+uds:///tmp/test_byo_ipc.sock";
        let server = match IpcServer::<String>::bind(URL) {
            Ok(server) => server,
            Err(e) => match e {
                IpcError::IoError(ref e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                    // delete the socket file and try again
                    std::fs::remove_file(FILE).unwrap();
                    IpcServer::<String>::bind(URL).unwrap()
                }
                _ => Err(e).unwrap(),
            },
        };

        thread::spawn(move || {
            for conn in server.incoming() {
                thread::spawn(move || {
                    let conn = conn.unwrap();
                    let message = conn.recv().unwrap();
                    assert_eq!(message, "hello");
                    conn.send(&"world".to_string()).unwrap();
                });
            }
        });

        let client = IpcConnection::<String>::open(URL).unwrap();
        client.send(&"hello".to_string()).unwrap();
        let message = client.recv().unwrap();
        assert_eq!(message, "world");

        // delete the socket file, since we're well-behaved citizens :)
        std::fs::remove_file(FILE).unwrap();
    }

    #[test]
    #[cfg(any(windows, target_os = "linux"))]
    fn test_namespace() {
        #[cfg(windows)]
        const URL: &str = "msgpack+namespace://./pipe/test_byo_ipc";

        #[cfg(target_os = "linux")]
        const URL: &str = "msgpack+namespace:///test_byo_ipc";

        let server = IpcServer::<String>::bind(URL).unwrap();

        thread::spawn(move || {
            for conn in server.incoming() {
                thread::spawn(move || {
                    let conn = conn.unwrap();
                    let message = conn.recv().unwrap();
                    assert_eq!(message, "hello");
                    conn.send(&"world".to_string()).unwrap();
                });
            }
        });

        let client = IpcConnection::<String>::open(URL).unwrap();
        client.send(&"hello".to_string()).unwrap();
        let message = client.recv().unwrap();
        assert_eq!(message, "world");
    }

    #[test]
    fn test_tcp() {
        const URL: &str = "msgpack+tcp://localhost:12345";
        let server = IpcServer::<String>::bind(URL).unwrap();

        thread::spawn(move || {
            for conn in server.incoming() {
                thread::spawn(move || {
                    let conn = conn.unwrap();
                    let message = conn.recv().unwrap();
                    assert_eq!(message, "hello");
                    conn.send(&"world".to_string()).unwrap();
                });
            }
        });

        let client = IpcConnection::<String>::open(URL).unwrap();
        client.send(&"hello".to_string()).unwrap();
        let message = client.recv().unwrap();
        assert_eq!(message, "world");
    }

    #[test]
    fn test_multiple_messages() {
        const URL: &str = "msgpack+tcp://localhost:12346"; // different port
        let server = IpcServer::<String>::bind(URL).unwrap();

        thread::spawn(move || {
            for conn in server.incoming() {
                thread::spawn(move || {
                    let conn = conn.unwrap();
                    while let Ok(message) = conn.recv() {
                        conn.send(&message.to_uppercase()).unwrap();
                    }
                });
            }
        });

        let client = IpcConnection::<String>::open(URL).unwrap();
        client.send(&"hello".to_string()).unwrap();
        let message = client.recv().unwrap();
        assert_eq!(message, "HELLO");
        client.send(&"world".to_string()).unwrap();
        let message = client.recv().unwrap();
        assert_eq!(message, "WORLD");
    }

    #[test]
    fn test_multiple_clients() {
        const URL: &str = "msgpack+tcp://localhost:12347"; // different port
        let server = IpcServer::<String>::bind(URL).unwrap();

        thread::spawn(move || {
            for conn in server.incoming() {
                thread::spawn(move || {
                    let conn = conn.unwrap();
                    let message = conn.recv().unwrap();
                    conn.send(&message.to_uppercase()).unwrap();
                });
            }
        });

        let client1 = IpcConnection::<String>::open(URL).unwrap();
        client1.send(&"hello".to_string()).unwrap();
        let message = client1.recv().unwrap();
        assert_eq!(message, "HELLO");

        let client2 = IpcConnection::<String>::open(URL).unwrap();
        client2.send(&"world".to_string()).unwrap();
        let message = client2.recv().unwrap();
        assert_eq!(message, "WORLD");
    }
}
