#[derive(Debug, Default, Clone)]
pub struct LatencyStats {
    samples: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencySummary {
    pub count: usize,
    pub avg_us: u64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

impl LatencyStats {
    pub fn add_sample(&mut self, duration_us: u64) {
        self.samples.push(duration_us);
    }

    pub fn summary(&self) -> Option<LatencySummary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut values = self.samples.clone();
        values.sort_unstable();
        let count = values.len();
        let sum: u128 = values.iter().map(|&v| v as u128).sum();
        let avg_us = ((sum as f64) / (count as f64)).round() as u64;
        let p50_us = percentile(&values, 0.5);
        let p90_us = percentile(&values, 0.9);
        let p99_us = percentile(&values, 0.99);
        let max_us = *values.last().unwrap_or(&0);
        Some(LatencySummary {
            count,
            avg_us,
            p50_us,
            p90_us,
            p99_us,
            max_us,
        })
    }
}

fn percentile(sorted_values: &[u64], p: f64) -> u64 {
    let len = sorted_values.len();
    if len == 0 {
        return 0;
    }
    let idx = ((len - 1) as f64 * p + 0.5).floor() as usize;
    sorted_values[idx.min(len - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_summary() {
        let mut stats = LatencyStats::default();
        stats.add_sample(10);
        stats.add_sample(20);
        stats.add_sample(30);

        let summary = stats.summary().expect("summary");
        assert_eq!(summary.count, 3);
        assert_eq!(summary.avg_us, 20);
        assert_eq!(summary.p50_us, 20);
        assert_eq!(summary.p90_us, 30);
        assert_eq!(summary.p99_us, 30);
        assert_eq!(summary.max_us, 30);
    }

    #[test]
    fn test_percentile_rounding() {
        let mut stats = LatencyStats::default();
        for value in [5_u64, 10, 15, 20] {
            stats.add_sample(value);
        }
        let summary = stats.summary().expect("summary");
        assert_eq!(summary.p50_us, 15);
        assert_eq!(summary.p90_us, 20);
    }
}
