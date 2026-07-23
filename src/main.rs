use toolkit_rs::painc::{PaincConf, set_panic_handler};
#[tokio::main]
async fn main() {
    set_panic_handler(PaincConf {
        version: "1.0.0".into(),
        build_time: "".into(),
        painc_exit: true,
    });

    match supervisord::cli().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("err:{}", e)
        }
    }
}
