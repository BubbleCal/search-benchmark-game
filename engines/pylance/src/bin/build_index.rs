use std::env;
use std::io::BufReader;

use pylance_engine::build_index;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let idx_path = args.next().unwrap_or_else(|| "idx".to_string());
    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin);
    build_index(reader, &idx_path, None).await?;
    Ok(())
}
