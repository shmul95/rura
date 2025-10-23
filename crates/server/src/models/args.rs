use clap::Parser;

#[derive(Parser, Debug)]
#[command(about = "Simple TCP server with configurable port")]
pub struct Args {
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    // TLS certificate (PEM). Required: server is TLS-only.
    #[arg(long, required = true)]
    pub tls_cert: String,

    // TLS private key (PEM; PKCS#8 or RSA). Required: server is TLS-only.
    #[arg(long, required = true)]
    pub tls_key: String,

    // Enable verbose debug logging of raw inbound/outbound JSON lines
    #[arg(long, default_value_t = false)]
    pub debug_io: bool,
}
