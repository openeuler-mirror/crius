/*
Copyright 2026 KylinSoft  Co., Ltd.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/


use std::unimplemented;
use std::{collections::HashMap, future::Future, io::Write};

use base64::Engine;
use nix::sys::termios::{self, SetArg, Termios};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use crate::crs::{
    args::{StreamOptions, StreamProtocolArg},
    error::{CliError, CommandResult},
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum StreamProtocol {
    Websocket,
    Spdy,
}

impl From<StreamProtocolArg> for StreamProtocol {
    fn from(value: StreamProtocolArg) -> Self {
        match value {
            StreamProtocolArg::Websocket => Self::Websocket,
            StreamProtocolArg::Spdy => Self::Spdy,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecStreamOptions {
    pub stream_url: Option<String>,
    pub container_id: String,
    pub command: Vec<String>,
    pub tty: bool,
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
    pub resize: Option<String>,
    pub protocol: StreamProtocol,
}

impl ExecStreamOptions {
    pub(crate) fn from_args(
        container_id: String,
        command: Vec<String>,
        stream: StreamOptions,
    ) -> Result<Self, CliError> {
        if container_id.trim().is_empty() {
            return Err(CliError::invalid_input("container id must not be empty")
                .with_command("crs container exec"));
        }
        if command.is_empty() {
            return Err(CliError::invalid_input("exec command must not be empty")
                .with_command("crs container exec"));
        }

        Ok(Self {
            stream_url: None,
            container_id,
            command,
            tty: stream.tty,
            stdin: stream.stdin,
            stdout: stream.stdout,
            stderr: stream.stderr && !stream.tty,
            resize: stream.resize,
            protocol: stream.protocol.into(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttachStreamOptions {
    pub stream_url: Option<String>,
    pub container_id: String,
    pub tty: bool,
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
    pub resize: Option<String>,
    pub protocol: StreamProtocol,
}

impl AttachStreamOptions {
    pub(crate) fn from_args(container_id: String, stream: StreamOptions) -> Result<Self, CliError> {
        if container_id.trim().is_empty() {
            return Err(CliError::invalid_input("container id must not be empty")
                .with_command("crs container attach"));
        }
        if !stream.stdin && !stream.stdout && !stream.stderr {
            return Err(CliError::invalid_input(
                "attach requires at least one of stdin, stdout, or stderr",
            )
            .with_command("crs container attach"));
        }

        Ok(Self {
            stream_url: None,
            container_id,
            tty: stream.tty,
            stdin: stream.stdin,
            stdout: stream.stdout,
            stderr: stream.stderr && !stream.tty,
            resize: stream.resize,
            protocol: stream.protocol.into(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PortForwardOptions {
    pub pod_id: String,
    pub stream_url: Option<String>,
    pub forwards: Vec<PortForwardSpec>,
    pub protocol: StreamProtocol,
}

impl PortForwardOptions {
    pub(crate) fn from_args(pod_id: String, forward: Vec<String>) -> Result<Self, CliError> {
        if pod_id.trim().is_empty() {
            return Err(CliError::invalid_input("pod id must not be empty")
                .with_command("crs pod port-forward"));
        }

        let forwards = forward
            .iter()
            .map(|value| parse_port_forward(value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|message| {
                CliError::invalid_input(message).with_command("crs pod port-forward")
            })?;

        if forwards.is_empty() {
            return Err(CliError::invalid_input(
                "pod port-forward requires at least one --forward LOCAL:REMOTE",
            )
            .with_command("crs pod port-forward"));
        }

        Ok(Self {
            pod_id,
            stream_url: None,
            forwards,
            protocol: StreamProtocol::Websocket,
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct PortForwardSpec {
    pub local: u16,
    pub remote: u16,
}

fn parse_port_forward(value: &str) -> Result<PortForwardSpec, String> {
    let (local, remote) = value
        .split_once(':')
        .ok_or_else(|| format!("invalid port forward \"{value}\": expected LOCAL:REMOTE"))?;

    Ok(PortForwardSpec {
        local: parse_forward_port(value, local)?,
        remote: parse_forward_port(value, remote)?,
    })
}

fn parse_forward_port(source: &str, value: &str) -> Result<u16, String> {
    let port = value
        .parse::<u16>()
        .map_err(|_| format!("invalid port forward \"{source}\": port must be 1-65535"))?;
    if port == 0 {
        return Err(format!(
            "invalid port forward \"{source}\": port must be 1-65535"
        ));
    }
    Ok(port)
}

pub(crate) async fn exec(options: ExecStreamOptions) -> Result<CommandResult, CliError> {
    let _raw_mode = SystemRawModeBackend.enter_raw_mode(options.tty)?;
    let resize = ResizeEvents::start(options.tty);
    let initial_size = resize.initial();
    let resize_events = options.tty.then(|| resize.into_receiver());
    let url = select_stream_url("exec", options.protocol, options.stream_url.as_deref())?;
    let routing = OutputRouting::exec(&options);
    let output = websocket_stream_with_writers(
        url,
        WebsocketIo::from_options(options.stdin, initial_size, resize_events),
        routing,
        &mut LocalStreamWriters,
        None,
    )
    .await?;
    Ok(CommandResult::from_code(
        output.exit_code.unwrap_or_default(),
    ))
}

pub(crate) async fn attach(options: AttachStreamOptions) -> Result<CommandResult, CliError> {
    attach_with_interrupt(options, tokio::signal::ctrl_c()).await
}

pub(crate) async fn attach_ready(
    options: AttachStreamOptions,
    ready: oneshot::Sender<Result<(), CliError>>,
) -> Result<CommandResult, CliError> {
    attach_with_interrupt_and_ready(options, tokio::signal::ctrl_c(), Some(ready)).await
}

pub(crate) async fn port_forward(options: PortForwardOptions) -> Result<CommandResult, CliError> {
    let _url = select_stream_url(
        "port-forward",
        options.protocol,
        options.stream_url.as_deref(),
    )?;
    let _listeners = bind_port_forward_listeners(&options.forwards).await?;
    Ok(CommandResult::success())
}

async fn attach_with_interrupt<I>(
    options: AttachStreamOptions,
    interrupt: I,
) -> Result<CommandResult, CliError>
where
    I: Future<Output = std::io::Result<()>>,
{
    attach_with_interrupt_and_ready(options, interrupt, None).await
}

async fn attach_with_interrupt_and_ready<I>(
    options: AttachStreamOptions,
    interrupt: I,
    ready: Option<oneshot::Sender<Result<(), CliError>>>,
) -> Result<CommandResult, CliError>
where
    I: Future<Output = std::io::Result<()>>,
{
    let _raw_mode = SystemRawModeBackend.enter_raw_mode(options.tty)?;
    let resize = ResizeEvents::start(options.tty);
    let initial_size = resize.initial();
    let resize_events = options.tty.then(|| resize.into_receiver());
    let url = select_stream_url("attach", options.protocol, options.stream_url.as_deref())?;
    let routing = OutputRouting::attach(&options);
    let mut writers = LocalStreamWriters;
    let stream = websocket_stream_with_writers(
        url,
        WebsocketIo::from_options(options.stdin, initial_size, resize_events),
        routing,
        &mut writers,
        ready,
    );
    tokio::pin!(stream);
    tokio::pin!(interrupt);

    tokio::select! {
        result = &mut stream => {
            let _output = result?;
            Ok(CommandResult::success())
        }
        result = &mut interrupt => {
            result.map_err(|source| CliError::internal(format!("failed to listen for interrupt: {source}")))?;
            Ok(CommandResult::failure(crate::crs::error::ExitStatus::Interrupted))
        }
    }
}

fn select_stream_url<'a>(
    operation: &str,
    protocol: StreamProtocol,
    stream_url: Option<&'a str>,
) -> Result<&'a str, CliError> {
    match protocol {
        StreamProtocol::Websocket => stream_url.ok_or_else(|| {
            unimplemented!("crs streaming {operation} URL resolution")
        }),
        StreamProtocol::Spdy => Err(CliError::internal(
            "SPDY streaming is not supported by crs yet; use --protocol websocket",
        )),
    }
}

async fn bind_port_forward_listeners(
    forwards: &[PortForwardSpec],
) -> Result<Vec<(PortForwardSpec, TcpListener)>, CliError> {
    let mut listeners = Vec::with_capacity(forwards.len());
    for forward in forwards {
        let listener = TcpListener::bind(("127.0.0.1", forward.local))
            .await
            .map_err(|source| {
                CliError::internal(format!(
                    "failed to bind local port {} for remote port {}: {source}",
                    forward.local, forward.remote
                ))
            })?;
        listeners.push((*forward, listener));
    }
    Ok(listeners)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub width: u16,
    pub height: u16,
}

struct ResizeEvents {
    initial: Option<TerminalSize>,
    receiver: mpsc::Receiver<TerminalSize>,
}

impl ResizeEvents {
    fn start(enabled: bool) -> Self {
        if !enabled {
            return Self::empty();
        }

        let fd = nix::libc::STDIN_FILENO;
        if !nix::unistd::isatty(fd).unwrap_or(false) {
            return Self::empty();
        }

        let initial = terminal_size(fd);
        let (sender, receiver) = mpsc::channel(8);
        tokio::spawn(async move {
            let Ok(mut signal) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
            else {
                return;
            };

            while signal.recv().await.is_some() {
                let Some(size) = terminal_size(fd) else {
                    continue;
                };
                if sender.send(size).await.is_err() {
                    break;
                }
            }
        });

        Self { initial, receiver }
    }

    fn empty() -> Self {
        let (_sender, receiver) = mpsc::channel(1);
        Self {
            initial: None,
            receiver,
        }
    }

    fn initial(&self) -> Option<TerminalSize> {
        self.initial
    }

    fn into_receiver(self) -> mpsc::Receiver<TerminalSize> {
        self.receiver
    }
}

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const WS_BINARY_OPCODE: u8 = 0x2;
const WS_CLOSE_OPCODE: u8 = 0x8;
const WS_PING_OPCODE: u8 = 0x9;
const WS_PONG_OPCODE: u8 = 0xa;
const WS_CHANNEL_STDIN: u8 = 0;
const WS_CHANNEL_STDOUT: u8 = 1;
const WS_CHANNEL_STDERR: u8 = 2;
const WS_CHANNEL_ERROR: u8 = 3;
const WS_CHANNEL_RESIZE: u8 = 4;
const REMOTE_COMMAND_PROTOCOLS: &[&str] = &[
    "v5.channel.k8s.io",
    "v4.channel.k8s.io",
    "v3.channel.k8s.io",
    "v2.channel.k8s.io",
    "channel.k8s.io",
];

#[derive(Debug, Default)]
pub(crate) struct WebsocketIo {
    pub stdin: WebsocketStdin,
    pub initial_size: Option<TerminalSize>,
    pub resize_events: Option<mpsc::Receiver<TerminalSize>>,
}

impl WebsocketIo {
    fn from_options(
        stdin: bool,
        initial_size: Option<TerminalSize>,
        resize_events: Option<mpsc::Receiver<TerminalSize>>,
    ) -> Self {
        Self {
            stdin: if stdin {
                WebsocketStdin::Local
            } else {
                WebsocketStdin::Disabled
            },
            initial_size,
            resize_events,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum WebsocketStdin {
    #[default]
    Disabled,
    Local,
    #[cfg_attr(not(test), allow(dead_code))]
    Buffered(Vec<u8>),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct WebsocketStreamOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct OutputRouting {
    stdout: bool,
    stderr: bool,
}

impl OutputRouting {
    fn exec(options: &ExecStreamOptions) -> Self {
        Self {
            stdout: options.stdout,
            stderr: options.stderr,
        }
    }

    fn attach(options: &AttachStreamOptions) -> Self {
        Self {
            stdout: options.stdout,
            stderr: options.stderr,
        }
    }
}

trait StreamWriters {
    fn stdout(&mut self, payload: &[u8]) -> Result<(), CliError>;
    fn stderr(&mut self, payload: &[u8]) -> Result<(), CliError>;
}

struct LocalStreamWriters;

impl StreamWriters for LocalStreamWriters {
    fn stdout(&mut self, payload: &[u8]) -> Result<(), CliError> {
        let mut stdout = std::io::stdout();
        stdout
            .write_all(payload)
            .map_err(|source| CliError::internal(format!("failed to write stdout: {source}")))?;
        stdout
            .flush()
            .map_err(|source| CliError::internal(format!("failed to flush stdout: {source}")))
    }

    fn stderr(&mut self, payload: &[u8]) -> Result<(), CliError> {
        let mut stderr = std::io::stderr();
        stderr
            .write_all(payload)
            .map_err(|source| CliError::internal(format!("failed to write stderr: {source}")))?;
        stderr
            .flush()
            .map_err(|source| CliError::internal(format!("failed to flush stderr: {source}")))
    }
}

async fn websocket_stream_with_writers<W>(
    url: &str,
    io: WebsocketIo,
    routing: OutputRouting,
    writers: &mut W,
    ready: Option<oneshot::Sender<Result<(), CliError>>>,
) -> Result<WebsocketStreamOutput, CliError>
where
    W: StreamWriters,
{
    let target = WebsocketTarget::parse(url)?;
    let mut stream = TcpStream::connect((target.host.as_str(), target.port))
        .await
        .map_err(|source| CliError::internal(format!("failed to connect websocket: {source}")))?;
    let key = websocket_client_key();
    let request = websocket_handshake_request(&target, &key);
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|source| {
            CliError::internal(format!("failed to send websocket handshake: {source}"))
        })?;
    stream.flush().await.map_err(|source| {
        CliError::internal(format!("failed to flush websocket handshake: {source}"))
    })?;

    let response = read_http_response(&mut stream).await?;
    if let Err(error) = validate_websocket_handshake(&response, &key) {
        if let Some(ready) = ready {
            let _ = ready.send(Err(CliError::internal(error.to_string())));
        }
        return Err(error);
    }
    let (reader, writer) = stream.into_split();
    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer));
    if let Some(size) = io.initial_size {
        let resize_result = {
            let mut writer = writer.lock().await;
            write_resize_frame(&mut *writer, size).await
        };
        match resize_result {
            Ok(()) => {
                if let Some(ready) = ready {
                    let _ = ready.send(Ok(()));
                }
            }
            Err(error) => {
                if let Some(ready) = ready {
                    let _ = ready.send(Err(CliError::internal(error.to_string())));
                }
                return Err(error);
            }
        }
    } else if let Some(ready) = ready {
        let _ = ready.send(Ok(()));
    }

    let mut tasks = Vec::new();
    start_stdin_pump(io.stdin, std::sync::Arc::clone(&writer), &mut tasks);
    start_resize_pump(
        None,
        io.resize_events,
        std::sync::Arc::clone(&writer),
        &mut tasks,
    );

    let mut output = WebsocketStreamOutput::default();
    let mut reader = reader;
    loop {
        let Some(frame) = read_websocket_frame(&mut reader).await? else {
            break;
        };
        match frame.opcode {
            WS_BINARY_OPCODE => {
                apply_remotecommand_frame(&mut output, writers, routing, &frame.payload)?
            }
            WS_CLOSE_OPCODE => break,
            WS_PING_OPCODE => {
                write_websocket_frame(
                    &mut *writer.lock().await,
                    WS_PONG_OPCODE,
                    &frame.payload,
                    true,
                )
                .await?;
            }
            _ => {}
        }
    }

    for task in tasks {
        task.abort();
    }

    Ok(output)
}

fn start_stdin_pump(
    stdin: WebsocketStdin,
    writer: std::sync::Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    match stdin {
        WebsocketStdin::Disabled => {}
        WebsocketStdin::Buffered(payload) => {
            tasks.push(tokio::spawn(async move {
                let mut writer = writer.lock().await;
                let _ =
                    write_websocket_channel_frame(&mut *writer, WS_CHANNEL_STDIN, &payload).await;
                let _ = write_websocket_channel_frame(&mut *writer, WS_CHANNEL_STDIN, &[]).await;
            }));
        }
        WebsocketStdin::Local => {
            tasks.push(tokio::spawn(async move {
                let mut stdin = tokio::io::stdin();
                let mut buffer = [0u8; 8192];
                loop {
                    let n = match stdin.read(&mut buffer).await {
                        Ok(0) => {
                            let mut writer = writer.lock().await;
                            let _ =
                                write_websocket_channel_frame(&mut *writer, WS_CHANNEL_STDIN, &[])
                                    .await;
                            break;
                        }
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    let mut writer = writer.lock().await;
                    if write_websocket_channel_frame(&mut *writer, WS_CHANNEL_STDIN, &buffer[..n])
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
    }
}

fn start_resize_pump(
    initial_size: Option<TerminalSize>,
    resize_events: Option<mpsc::Receiver<TerminalSize>>,
    writer: std::sync::Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    if initial_size.is_none() && resize_events.is_none() {
        return;
    }

    tasks.push(tokio::spawn(async move {
        if let Some(size) = initial_size {
            let mut writer = writer.lock().await;
            if write_resize_frame(&mut *writer, size).await.is_err() {
                return;
            }
        }

        let Some(mut resize_events) = resize_events else {
            return;
        };
        while let Some(size) = resize_events.recv().await {
            let mut writer = writer.lock().await;
            if write_resize_frame(&mut *writer, size).await.is_err() {
                break;
            }
        }
    }));
}

async fn write_resize_frame<W>(writer: &mut W, size: TerminalSize) -> Result<(), CliError>
where
    W: AsyncWrite + Unpin,
{
    let payload = format!(r#"{{"width":{},"height":{}}}"#, size.width, size.height);
    write_websocket_channel_frame(writer, WS_CHANNEL_RESIZE, payload.as_bytes()).await
}

async fn write_websocket_channel_frame<W>(
    writer: &mut W,
    channel: u8,
    payload: &[u8],
) -> Result<(), CliError>
where
    W: AsyncWrite + Unpin,
{
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(channel);
    frame.extend_from_slice(payload);
    write_websocket_frame(writer, WS_BINARY_OPCODE, &frame, true).await
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WebsocketTarget {
    host: String,
    port: u16,
    path_and_query: String,
}

impl WebsocketTarget {
    fn parse(url: &str) -> Result<Self, CliError> {
        let without_scheme = url
            .strip_prefix("ws://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or_else(|| {
                CliError::invalid_input("only ws:// and http:// streaming URLs are supported")
            })?;
        let (authority, path) = without_scheme
            .split_once('/')
            .map(|(authority, path)| (authority, format!("/{path}")))
            .unwrap_or((without_scheme, "/".to_string()));
        let (host, port) = authority
            .rsplit_once(':')
            .map(|(host, port)| {
                let port = port.parse::<u16>().map_err(|_| {
                    CliError::invalid_input(format!("invalid websocket URL port: {port}"))
                })?;
                Ok((host.to_string(), port))
            })
            .transpose()?
            .unwrap_or_else(|| (authority.to_string(), 80));
        if host.is_empty() {
            return Err(CliError::invalid_input(
                "websocket URL host must not be empty",
            ));
        }

        Ok(Self {
            host,
            port,
            path_and_query: path,
        })
    }

    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HttpResponseHead {
    status: u16,
    headers: HashMap<String, String>,
}

async fn read_http_response<R>(reader: &mut R) -> Result<HttpResponseHead, CliError>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        reader.read_exact(&mut byte).await.map_err(|source| {
            CliError::internal(format!("failed to read websocket response: {source}"))
        })?;
        bytes.push(byte[0]);
        if bytes.len() > 16 * 1024 {
            return Err(CliError::internal(
                "websocket response headers are too large",
            ));
        }
    }
    let response = String::from_utf8(bytes)
        .map_err(|source| CliError::internal(format!("invalid websocket response: {source}")))?;
    let mut lines = response.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| CliError::internal("missing websocket response status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| CliError::internal("invalid websocket response status line"))?;

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Ok(HttpResponseHead { status, headers })
}

fn validate_websocket_handshake(response: &HttpResponseHead, key: &str) -> Result<(), CliError> {
    if response.status != 101 {
        return Err(CliError::internal(format!(
            "websocket handshake failed with HTTP {}",
            response.status
        )));
    }
    let expected_accept = websocket_accept_value(key);
    let accept = response
        .headers
        .get("sec-websocket-accept")
        .map(String::as_str)
        .unwrap_or_default();
    if accept != expected_accept {
        return Err(CliError::internal(
            "websocket handshake returned invalid accept value",
        ));
    }
    let protocol = response
        .headers
        .get("sec-websocket-protocol")
        .map(String::as_str)
        .unwrap_or_default();
    if !REMOTE_COMMAND_PROTOCOLS.contains(&protocol) {
        return Err(CliError::internal(
            "websocket handshake did not negotiate a remotecommand protocol",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WebsocketFrame {
    opcode: u8,
    payload: Vec<u8>,
}

async fn read_websocket_frame<R>(reader: &mut R) -> Result<Option<WebsocketFrame>, CliError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 2];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(source) => {
            return Err(CliError::internal(format!(
                "failed to read websocket frame: {source}"
            )));
        }
    }

    let opcode = header[0] & 0x0f;
    let masked = (header[1] & 0x80) != 0;
    let mut payload_len = (header[1] & 0x7f) as u64;
    if payload_len == 126 {
        let mut extended = [0u8; 2];
        reader.read_exact(&mut extended).await.map_err(|source| {
            CliError::internal(format!("failed to read websocket frame length: {source}"))
        })?;
        payload_len = u16::from_be_bytes(extended) as u64;
    } else if payload_len == 127 {
        let mut extended = [0u8; 8];
        reader.read_exact(&mut extended).await.map_err(|source| {
            CliError::internal(format!("failed to read websocket frame length: {source}"))
        })?;
        payload_len = u64::from_be_bytes(extended);
    }

    let mut mask = [0u8; 4];
    if masked {
        reader.read_exact(&mut mask).await.map_err(|source| {
            CliError::internal(format!("failed to read websocket frame mask: {source}"))
        })?;
    }

    let mut payload = vec![0u8; payload_len as usize];
    if payload_len > 0 {
        reader.read_exact(&mut payload).await.map_err(|source| {
            CliError::internal(format!("failed to read websocket frame payload: {source}"))
        })?;
    }
    if masked {
        apply_websocket_mask(&mut payload, mask);
    }

    Ok(Some(WebsocketFrame { opcode, payload }))
}

async fn write_websocket_frame<W>(
    writer: &mut W,
    opcode: u8,
    payload: &[u8],
    masked: bool,
) -> Result<(), CliError>
where
    W: AsyncWrite + Unpin,
{
    let mut header = vec![0x80 | (opcode & 0x0f)];
    let mask_bit = if masked { 0x80 } else { 0 };
    if payload.len() < 126 {
        header.push(mask_bit | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        header.push(mask_bit | 126);
        header.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        header.push(mask_bit | 127);
        header.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }

    let mut payload = payload.to_vec();
    if masked {
        let mask = [0x12, 0x34, 0x56, 0x78];
        header.extend_from_slice(&mask);
        apply_websocket_mask(&mut payload, mask);
    }

    writer.write_all(&header).await.map_err(|source| {
        CliError::internal(format!("failed to write websocket frame header: {source}"))
    })?;
    if !payload.is_empty() {
        writer.write_all(&payload).await.map_err(|source| {
            CliError::internal(format!("failed to write websocket frame payload: {source}"))
        })?;
    }
    writer.flush().await.map_err(|source| {
        CliError::internal(format!("failed to flush websocket frame: {source}"))
    })?;
    Ok(())
}

fn apply_remotecommand_frame(
    output: &mut WebsocketStreamOutput,
    writers: &mut impl StreamWriters,
    routing: OutputRouting,
    payload: &[u8],
) -> Result<(), CliError> {
    let Some((&channel, payload)) = payload.split_first() else {
        return Ok(());
    };
    match channel {
        WS_CHANNEL_STDOUT => {
            output.stdout.extend_from_slice(payload);
            if routing.stdout {
                writers.stdout(payload)?;
            }
        }
        WS_CHANNEL_STDERR => {
            output.stderr.extend_from_slice(payload);
            if routing.stderr {
                writers.stderr(payload)?;
            }
        }
        WS_CHANNEL_ERROR => {
            if let Some(exit_code) = parse_remote_exit_code(payload) {
                output.exit_code = Some(exit_code);
            } else {
                return Err(CliError::internal(format!(
                    "remote stream error: {}",
                    String::from_utf8_lossy(payload)
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_remote_exit_code(payload: &[u8]) -> Option<i32> {
    let text = std::str::from_utf8(payload).ok()?.trim();
    if let Some(code) = text
        .strip_prefix("command terminated with non-zero exit code: ")
        .and_then(|code| code.parse::<i32>().ok())
    {
        return Some(code);
    }

    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value
        .pointer("/details/causes")
        .and_then(serde_json::Value::as_array)
        .and_then(|causes| {
            causes.iter().find_map(|cause| {
                let reason = cause.get("reason").and_then(serde_json::Value::as_str)?;
                (reason == "ExitCode")
                    .then(|| cause.get("message").and_then(serde_json::Value::as_str))
                    .flatten()
                    .and_then(|message| message.parse::<i32>().ok())
            })
        })
        .or_else(|| {
            value
                .get("exitCode")
                .and_then(serde_json::Value::as_i64)
                .and_then(|code| i32::try_from(code).ok())
        })
}

fn websocket_handshake_request(target: &WebsocketTarget, key: &str) -> String {
    format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Protocol: {}\r\n\r\n",
        target.path_and_query,
        target.authority(),
        key,
        REMOTE_COMMAND_PROTOCOLS.join(", ")
    )
}

fn websocket_client_key() -> String {
    base64::engine::general_purpose::STANDARD.encode(b"crius-crs-stream")
}

fn websocket_accept_value(key: &str) -> String {
    let mut payload = key.as_bytes().to_vec();
    payload.extend_from_slice(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(sha1_digest(&payload))
}

fn apply_websocket_mask(payload: &mut [u8], mask: [u8; 4]) {
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
}

fn sha1_digest(input: &[u8]) -> [u8; 20] {
    fn left_rotate(value: u32, bits: u32) -> u32 {
        (value << bits) | (value >> (32 - bits))
    }

    let mut message = input.to_vec();
    let bit_len = (message.len() as u64) * 8;
    message.push(0x80);
    while (message.len() % 64) != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    for chunk in message.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).take(16).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = left_rotate(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => (((b & c) | ((!b) & d)), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => (((b & c) | (b & d) | (c & d)), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = left_rotate(a, 5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = left_rotate(b, 30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn terminal_size(fd: i32) -> Option<TerminalSize> {
    let mut winsize = nix::libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe { nix::libc::ioctl(fd, nix::libc::TIOCGWINSZ, &mut winsize) };
    if rc != 0 || winsize.ws_col == 0 || winsize.ws_row == 0 {
        return None;
    }

    Some(TerminalSize {
        width: winsize.ws_col,
        height: winsize.ws_row,
    })
}

trait RawModeBackend {
    type Guard;

    fn enter_raw_mode(&self, enabled: bool) -> Result<Option<Self::Guard>, CliError>;
}

struct SystemRawModeBackend;

impl RawModeBackend for SystemRawModeBackend {
    type Guard = TtyRawModeGuard;

    fn enter_raw_mode(&self, enabled: bool) -> Result<Option<Self::Guard>, CliError> {
        TtyRawModeGuard::enter(enabled)
    }
}

struct TtyRawModeGuard {
    fd: i32,
    original: Termios,
}

impl TtyRawModeGuard {
    fn enter(enabled: bool) -> Result<Option<Self>, CliError> {
        if !enabled {
            return Ok(None);
        }

        let fd = nix::libc::STDIN_FILENO;
        if !nix::unistd::isatty(fd).unwrap_or(false) {
            return Ok(None);
        }

        let original = termios::tcgetattr(fd).map_err(|source| {
            CliError::internal(format!("failed to read terminal mode: {source}"))
        })?;
        let mut raw = original.clone();
        termios::cfmakeraw(&mut raw);
        termios::tcsetattr(fd, SetArg::TCSANOW, &raw).map_err(|source| {
            CliError::internal(format!("failed to enter terminal raw mode: {source}"))
        })?;

        Ok(Some(Self { fd, original }))
    }
}

impl Drop for TtyRawModeGuard {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(self.fd, SetArg::TCSANOW, &self.original);
    }
}