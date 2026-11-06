extern crate mediabbs;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => mediabbs::run().await,
        _ => std::process::exit(mediabbs::cli::run(&args)),
    }
}
