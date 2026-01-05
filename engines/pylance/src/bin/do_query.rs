use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, Write};

use pylance_engine::{
    count_query_with_latency, open_dataset, run_topk_with_latency, topk_count_with_latency,
    LatencyStats,
};

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

struct LatencyLog {
    writer: BufWriter<File>,
    stats: BTreeMap<String, LatencyStats>,
    active: bool,
}

impl LatencyLog {
    fn from_env() -> io::Result<Option<Self>> {
        let Ok(path) = env::var("PYLANCE_LATENCY_LOG") else {
            return Ok(None);
        };
        if path.trim().is_empty() {
            return Ok(None);
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        writeln!(
            writer,
            "command,num_queries,avg_us,p50_us,p90_us,p99_us,max_us"
        )?;
        Ok(Some(Self {
            writer,
            stats: BTreeMap::new(),
            active: false,
        }))
    }

    fn activate(&mut self) {
        self.active = true;
    }

    fn record(&mut self, command: &str, duration_us: u64) {
        if !self.active {
            return;
        }
        self.stats
            .entry(command.to_string())
            .or_default()
            .add_sample(duration_us);
    }

    fn finish(&mut self) -> io::Result<()> {
        for (command, stats) in &self.stats {
            let Some(summary) = stats.summary() else {
                continue;
            };
            writeln!(
                self.writer,
                "{},{},{},{},{},{},{}",
                command,
                summary.count,
                summary.avg_us,
                summary.p50_us,
                summary.p90_us,
                summary.p99_us,
                summary.max_us
            )?;
        }
        self.writer.flush()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let idx_path = args.next().unwrap_or_else(|| "idx".to_string());
    let dataset = open_dataset(&idx_path).await?;
    let mut latency_log = LatencyLog::from_env()?;

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

        if command == "PYLANCE_LOG_START" {
            if let Some(logger) = latency_log.as_mut() {
                logger.activate();
            }
            writeln!(stdout, "OK")?;
            stdout.flush()?;
            continue;
        }

        match parse_command(command) {
            Some(Command::Count) => {
                let (result, duration_us) = count_query_with_latency(&dataset, query).await?;
                writeln!(stdout, "{result}")?;
                if let Some(logger) = latency_log.as_mut() {
                    logger.record(command, duration_us);
                }
            }
            Some(Command::TopK(k)) => {
                let duration_us = run_topk_with_latency(&dataset, query, k).await?;
                writeln!(stdout, "1")?;
                if let Some(logger) = latency_log.as_mut() {
                    logger.record(command, duration_us);
                }
            }
            Some(Command::TopKCount(k)) => {
                let (result, duration_us) = topk_count_with_latency(&dataset, query, k).await?;
                writeln!(stdout, "{result}")?;
                if let Some(logger) = latency_log.as_mut() {
                    logger.record(command, duration_us);
                }
            }
            None => {
                writeln!(stdout, "UNSUPPORTED")?;
            }
        }
        stdout.flush()?;
        if env::var("PYLANCE_ASSERT_CACHE_HIT").ok().as_deref() == Some("1") {
            assert!(dataset.index_cache_hit_rate().await >= 1.0);
        }
    }

    if let Some(logger) = latency_log.as_mut() {
        logger.finish()?;
    }

    Ok(())
}
