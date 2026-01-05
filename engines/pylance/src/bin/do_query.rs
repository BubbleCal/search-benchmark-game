use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, Write};

use pylance_engine::{
    analyze_count_plan, analyze_topk_plan, count_query_with_latency, open_dataset,
    run_topk_with_latency, topk_count_with_latency, LatencyStats,
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

struct PlanLog {
    writer: BufWriter<File>,
    seen: HashSet<String>,
    active: bool,
}

impl PlanLog {
    fn from_env() -> io::Result<Option<Self>> {
        let Ok(path) = env::var("PYLANCE_ANALYZE_PLAN_LOG") else {
            return Ok(None);
        };
        if path.trim().is_empty() {
            return Ok(None);
        }
        let file = File::create(path)?;
        Ok(Some(Self {
            writer: BufWriter::new(file),
            seen: HashSet::new(),
            active: false,
        }))
    }

    fn activate(&mut self) {
        self.active = true;
    }

    fn should_log(&mut self, command: &str, query: &str) -> bool {
        if !self.active {
            return false;
        }
        let key = format!("{command}\t{query}");
        self.seen.insert(key)
    }

    fn write_plan(&mut self, command: &str, query: &str, plan: &str) -> io::Result<()> {
        writeln!(self.writer, "=== {command}\\t{query} ===")?;
        writeln!(self.writer, "{plan}")?;
        writeln!(self.writer, "---")?;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let idx_path = args.next().unwrap_or_else(|| "idx".to_string());
    let dataset = open_dataset(&idx_path).await?;
    let mut latency_log = LatencyLog::from_env()?;
    let mut plan_log = PlanLog::from_env()?;

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
            if let Some(logger) = plan_log.as_mut() {
                logger.activate();
            }
            writeln!(stdout, "OK")?;
            stdout.flush()?;
            continue;
        }

        let command_kind = match parse_command(command) {
            Some(kind) => kind,
            None => {
                writeln!(stdout, "UNSUPPORTED")?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(logger) = plan_log.as_mut() {
            if logger.should_log(command, query) {
                let plan = match command_kind {
                    Command::Count => analyze_count_plan(&dataset, query).await?,
                    Command::TopK(k) => analyze_topk_plan(&dataset, query, k).await?,
                    Command::TopKCount(_) => analyze_count_plan(&dataset, query).await?,
                };
                match plan {
                    Some(plan) => logger.write_plan(command, query, &plan)?,
                    None => logger.write_plan(command, query, "EMPTY_QUERY")?,
                }
            }
        }

        match command_kind {
            Command::Count => {
                let (result, duration_us) = count_query_with_latency(&dataset, query).await?;
                writeln!(stdout, "{result}")?;
                if let Some(logger) = latency_log.as_mut() {
                    logger.record(command, duration_us);
                }
            }
            Command::TopK(k) => {
                let duration_us = run_topk_with_latency(&dataset, query, k).await?;
                writeln!(stdout, "1")?;
                if let Some(logger) = latency_log.as_mut() {
                    logger.record(command, duration_us);
                }
            }
            Command::TopKCount(k) => {
                let (result, duration_us) = topk_count_with_latency(&dataset, query, k).await?;
                writeln!(stdout, "{result}")?;
                if let Some(logger) = latency_log.as_mut() {
                    logger.record(command, duration_us);
                }
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
    if let Some(logger) = plan_log.as_mut() {
        logger.finish()?;
    }

    Ok(())
}
