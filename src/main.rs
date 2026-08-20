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

use anyhow::Error;
use clap::Parser;
use tracing::info;
use tonic::transport::Server;
use tokio_stream::wrappers::UnixListenerStream;
use tokio::net::UnixListener as TokioUnixListener;

use crius::config::Config;

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

    let listen = config.api.listen.clone();

    let server = Server::builder();    

    // 创建gRPC服务器
    info!("Starting crius gRPC server on {}", listen);

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
        serve_result?;
    } else {
        let addr: SocketAddr = listen.parse()?;
        let serve_result = server.serve_with_shutdown(addr, shutdown_signal()).await;
        serve_result?;
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