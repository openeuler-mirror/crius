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

use std::fs;
use std::path::{Path, PathBuf};
use std::result::Result::Ok;
use std::os::unix::net::UnixListener;
use std::os::unix::fs::symlink;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::Error;
use clap::Parser;
use tracing::{info, debug, warn};
use tonic::transport::Server;
use tonic_reflection::server::Builder as ReflectionBuilder;
use tokio_stream::wrappers::UnixListenerStream;
use tokio::net::UnixListener as TokioUnixListener;
use tracing_subscriber::{fmt, fmt::MakeWriter, util::SubscriberInitExt, prelude::__tracing_subscriber_SubscriberExt, EnvFilter};

use crius::proto::runtime::v1::{runtime_service_server::RuntimeServiceServer, image_service_server::ImageServiceServer};

use crius::config::Config;
use crius::defaults::{LOCAL_LOG_TIME_FORMAT, SERVER_SHUTDOWN_GRACE_PERIOD};
use crius::server::service::{RuntimeServiceImpl, RuntimeServiceConfig};

/// crius - OCI-based implementation of Kubernetes Container Runtime Interface
#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[clap(short, long, default_value = "/etc/crius/crius.conf")]
    config: PathBuf,

    /// Enable debug logging
    #[clap(short, long)]
    debug: bool,

    /// Log file path
    #[clap(short, long)]
    log: Option<PathBuf>,

    /// Listen address (IP:port or unix://path/to/socket). TCP requires api.allow_tcp_service=true.
    #[clap(long)]
    listen: Option<String>,

    /// Override the OCI runtime binary path.
    #[clap(long)]
    runtime_path: Option<PathBuf>,

    /// Override the runtime-specific config file path.
    #[clap(long)]
    runtime_config_path: Option<PathBuf>,

    /// Override the runtime state/runroot directory.
    #[clap(long)]
    runtime_root: Option<PathBuf>,

    /// Override the pause image reference used for PodSandbox.
    #[clap(long)]
    pause_image: Option<String>,

    /// Override CNI config directories, comma-separated.
    #[clap(long, value_delimiter = ',')]
    cni_config_dirs: Vec<String>,

    /// Override CNI plugin directories, comma-separated.
    #[clap(long, value_delimiter = ',')]
    cni_plugin_dirs: Vec<String>,

    /// Override the streaming server bind address.
    #[clap(long)]
    stream_address: Option<String>,

    /// Override the streaming server bind port.
    #[clap(long)]
    stream_port: Option<u16>,

    /// Override whether the streaming server uses TLS.
    #[clap(long)]
    stream_enable_tls: Option<bool>,

    /// Override the streaming TLS certificate file path.
    #[clap(long)]
    stream_tls_cert_file: Option<PathBuf>,

    /// Override the streaming TLS private key file path.
    #[clap(long)]
    stream_tls_key_file: Option<PathBuf>,

    /// Override the streaming TLS client CA file path.
    #[clap(long)]
    stream_tls_ca_file: Option<PathBuf>,

    /// Override the streaming TLS minimum version.
    #[clap(long)]
    stream_tls_min_version: Option<String>,

    /// Override the streaming TLS cipher suite list, comma-separated.
    #[clap(long, value_delimiter = ',')]
    stream_tls_cipher_suites: Vec<String>,

    /// Print the built-in default configuration as TOML and exit.
    #[clap(long, conflicts_with = "write_default_config")]
    dump_default_config: bool,

    /// Write the built-in default configuration as TOML to the given path and exit.
    #[clap(long, value_name = "PATH", conflicts_with = "dump_default_config")]
    write_default_config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();
    
    let mut config = match Config::load(&args.config) {
        Ok(cfg) => cfg,
        Err(crius::error::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            Config::default()
        }
        Err(err) => return Err(err.into()),
    };
    config.apply_env_overrides()?;
    apply_cli_overrides(&args, &mut config);

    init_logging(&config)?;

    info!("Loaded configuration from {}", args.config.display());
    
    let runtime_config = RuntimeServiceConfig::new(config.clone());
    let listen = config.api.listen.clone();
    // 创建服务实例
    let runtime_service =
        RuntimeServiceImpl::new(runtime_config.clone());
    let image_service = runtime_service.image_service();
    let reflection_service = ReflectionBuilder::configure()
        .register_encoded_file_descriptor_set(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/file_descriptor_set.bin"
        )))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create reflection service: {}", e))?;
    
    let runtime_service_server = RuntimeServiceServer::new(runtime_service.clone())
        .max_encoding_message_size(runtime_config.grpc_max_send_msg_size as usize)
        .max_decoding_message_size(runtime_config.grpc_max_recv_msg_size as usize);
    let image_service_server = ImageServiceServer::new(image_service)
        .max_encoding_message_size(runtime_config.grpc_max_send_msg_size as usize)
        .max_decoding_message_size(runtime_config.grpc_max_recv_msg_size as usize);

    // 注册路由
    let server = Server::builder()
        .add_service(runtime_service_server)
        .add_service(reflection_service)
        .add_service(image_service_server);    

    let shutdown_watchdog = spawn_shutdown_watchdog();

    // 创建gRPC服务器
    info!("Starting crius gRPC server on {}", listen);
    debug!("Using configuration: {:?}", runtime_config);

    if listen.starts_with("unix://") {
        // Unix domain socket
        let socket_path = listen.trim_start_matches("unix://");
        let path = Path::new(socket_path);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 清理旧socket文件
        let _ = fs::remove_file(path);

        // 创建Unix监听器
        let uds = UnixListener::bind(path)?;
        create_unix_socket_aliases(path, &config.api.listen_aliases)?;
        for alias in &config.api.listen_aliases {
            info!("CRI unix socket alias listening on {}", alias);
        }
        let uds_stream = UnixListenerStream::new(TokioUnixListener::from_std(uds)?);

        let serve_result = server
            .serve_with_incoming_shutdown(uds_stream, shutdown_signal()).await;
        shutdown_watchdog.abort();
        shutdown_runtime_service(&runtime_config.clean_shutdown_file).await;
        serve_result?;
    } else {
        let addr: SocketAddr = listen.parse()?;
        let serve_result = server.serve_with_shutdown(addr, shutdown_signal()).await;
        serve_result?;
        shutdown_watchdog.abort();
        shutdown_runtime_service(&runtime_config.clean_shutdown_file).await;
    }

    Ok(())
}


fn apply_cli_overrides(args: &Args, config: &mut Config) {
    if let Some(listen) = &args.listen {
        config.api.listen = listen.clone();
    }
    if let Some(log_file) = &args.log {
        config.logging.file = Some(log_file.display().to_string());
    }
    if args.debug {
        config.logging.level = "debug".to_string();
    }
}

fn unix_socket_path(listen: &str) -> Option<&Path> {
    listen.strip_prefix("unix://").map(Path::new)
}

fn create_unix_socket_aliases(primary_socket_path: &Path, aliases: &[String]) -> Result<(), Error> {
    for alias in aliases {
        let alias_path = unix_socket_path(alias).ok_or_else(|| {
            anyhow::anyhow!("api.listen_aliases value {} must use unix://", alias)
        })?;
        if alias_path == primary_socket_path {
            continue;
        }
        if let Some(parent) = alias_path.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::symlink_metadata(alias_path) {
            Ok(metadata) => {
                if metadata.file_type().is_dir() {
                    return Err(anyhow::anyhow!(
                        "socket alias {} points to a directory",
                        alias_path.display()
                    ));
                }
                fs::remove_file(alias_path)?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        symlink(primary_socket_path, alias_path)?;
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LocalLogTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalLogTimer {
    fn format_time(
        &self,
        writer: &mut tracing_subscriber::fmt::format::Writer<'_>,
    ) -> std::fmt::Result {
        write!(
            writer,
            "{}",
            chrono::Local::now().format(LOCAL_LOG_TIME_FORMAT)
        )
    }
}

#[derive(Clone)]
enum LogOutput {
    Stderr,
    File(Arc<Mutex<std::fs::File>>),
}

impl LogOutput {
    fn from_config(config: &Config) -> Result<Self, Error> {
        if let Some(path) = &config.logging.file {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            Ok(Self::File(Arc::new(Mutex::new(file))))
        } else {
            Ok(Self::Stderr)
        }
    }
}

struct LockedFileWriter {
    file: Arc<Mutex<std::fs::File>>,
}

impl std::io::Write for LockedFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| std::io::Error::other("log file mutex poisoned"))?;
        std::io::Write::write(&mut *file, buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| std::io::Error::other("log file mutex poisoned"))?;
        std::io::Write::flush(&mut *file)
    }
}

impl<'a> MakeWriter<'a> for LogOutput {
    type Writer = Box<dyn std::io::Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        match self {
            Self::Stderr => Box::new(std::io::stderr()),
            Self::File(file) => Box::new(LockedFileWriter { file: file.clone() }),
        }
    }
}

fn init_logging(config: &Config) -> Result<(), Error> {
    let base_filter = EnvFilter::try_from_default_env().or_else(|_| {
        EnvFilter::try_new(format!(
            "crius={},tower_http=info",
            config.logging.level.trim()
        ))
    })?;
    let writer = LogOutput::from_config(config)?;
    let fmt_layer = fmt::layer()
        .with_timer(LocalLogTimer)
        .with_file(true)
        .with_line_number(true)
        .with_writer(writer);

    tracing_subscriber::registry()
        .with(base_filter)
        .with(fmt_layer)
        .try_init()?;

    Ok(())
}

fn spawn_shutdown_watchdog() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        shutdown_signal().await;
        info!("Shutdown signal received, initiating CRI server shutdown");
        tokio::time::sleep(SERVER_SHUTDOWN_GRACE_PERIOD).await;
        warn!(
            "CRI server did not stop within {:?}; forcing process exit",
            SERVER_SHUTDOWN_GRACE_PERIOD
        );
        std::process::exit(0);
    })
}

async fn write_clean_shutdown_marker(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, b"clean\n").await?;
    Ok(())
}

async fn shutdown_runtime_service(clean_shutdown_file: &Path) {
    if let Err(err) = write_clean_shutdown_marker(clean_shutdown_file).await {
        log::error!("Failed to persist clean shutdown marker: {}", err);
    }
}