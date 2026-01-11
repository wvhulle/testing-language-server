use assert_lsp::server;

fn main() {
    env_logger::init();

    if let Err(ls_error) = server::run() {
        log::error!("Error: {:?}", ls_error);
    }
}
