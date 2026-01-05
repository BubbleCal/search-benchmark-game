use std::env;
use std::io::{self, BufRead, Write};

use lancedb_engine::{count_query, open_table, run_topk, topk_count};

#[derive(Debug)]
enum Command {
    Count,
    TopK(usize),
    TopKCount(usize),
}

fn parse_command(input: &str) -> Option<Command> {
    if input == "COUNT" {
        return Some(Command::Count);
    }
    if let Some(rest) = input.strip_prefix("TOP_") {
        if let Some(number) = rest.strip_suffix("_COUNT") {
            let k = number.parse::<usize>().ok()?;
            return Some(Command::TopKCount(k));
        }
        let k = rest.parse::<usize>().ok()?;
        return Some(Command::TopK(k));
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let idx_path = args.next().unwrap_or_else(|| "idx".to_string());
    let table = open_table(&idx_path).await?;

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let Some((command, query)) = line.split_once('\t') else {
            writeln!(stdout, "UNSUPPORTED")?;
            stdout.flush()?;
            continue;
        };

        match parse_command(command) {
            Some(Command::Count) => {
                let result = count_query(&table, query).await?;
                writeln!(stdout, "{}", result)?;
            }
            Some(Command::TopK(k)) => {
                run_topk(&table, query, k).await?;
                writeln!(stdout, "1")?;
            }
            Some(Command::TopKCount(k)) => {
                let result = topk_count(&table, query, k).await?;
                writeln!(stdout, "{}", result)?;
            }
            None => {
                writeln!(stdout, "UNSUPPORTED")?;
            }
        }
        stdout.flush()?;
    }

    Ok(())
}
