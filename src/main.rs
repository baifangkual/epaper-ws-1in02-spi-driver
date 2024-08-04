use env_logger::Target;
use log::{debug, error, info, LevelFilter, trace, warn};

mod pi;

fn main() {

    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .target(Target::Stdout)
        .init();

    info!("info log");
    debug!("debug log");
    trace!("trace log");
    warn!("warn log");
    // error!("error");

    println!("Hello, world!");
}
