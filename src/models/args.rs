use clap::Parser;

#[derive(Parser, Debug)]
#[command(about = "Simple TCP server with configurable port")]
pub struct Args {
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,
}