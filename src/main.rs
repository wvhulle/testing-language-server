use assert_lsp::{config, server};

fn main() {
    env_logger::init();
    config::init();

    if let Err(ls_error) = server::run() {
        log::error!("Error: {:?}", ls_error);
    }
}
