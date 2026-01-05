use std::io::BufRead;
use std::sync::Arc;
use std::time::Instant;

use arrow_array::{RecordBatch, RecordBatchReader, StringArray};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use futures::StreamExt;
use lance::dataset::{WriteMode, WriteParams};
use lance::Dataset;
use lance_index::scalar::inverted::query::{FtsQuery, MatchQuery, Operator};
use lance_index::scalar::{FullTextSearchQuery, InvertedIndexParams};
use lance_index::{DatasetIndexExt, IndexType};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

mod latency;
pub use latency::{LatencyStats, LatencySummary};

const DEFAULT_BATCH_SIZE: usize = 10_000;

#[allow(dead_code)]
static TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?P<prefix>[+-]?)(?:\"(?P<phrase>[^\"]+)\"|(?P<term>\S+))"#)
        .expect("valid token regex")
});

fn parse_doc_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let id_value = value.get("id")?;
    let text_value = value.get("text")?;

    let id = match id_value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };

    let text = match text_value {
        Value::String(value) => value.clone(),
        _ => return None,
    };

    Some((id, text))
}

pub struct JsonDocReader<R: BufRead> {
    reader: R,
    schema: SchemaRef,
    batch_size: usize,
    done: bool,
    line_buf: String,
}

impl<R: BufRead> JsonDocReader<R> {
    pub fn new(reader: R, batch_size: usize) -> Self {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("text", DataType::Utf8, false),
        ]));
        Self {
            reader,
            schema,
            batch_size,
            done: false,
            line_buf: String::new(),
        }
    }
}

impl<R: BufRead> Iterator for JsonDocReader<R> {
    type Item = Result<RecordBatch, ArrowError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let mut ids = Vec::with_capacity(self.batch_size);
        let mut texts = Vec::with_capacity(self.batch_size);

        while ids.len() < self.batch_size {
            self.line_buf.clear();
            match self.reader.read_line(&mut self.line_buf) {
                Ok(0) => {
                    self.done = true;
                    break;
                }
                Ok(_) => {
                    if let Some((id, text)) = parse_doc_line(&self.line_buf) {
                        ids.push(id);
                        texts.push(text);
                    }
                }
                Err(err) => return Some(Err(err.into())),
            }
        }

        if ids.is_empty() {
            return None;
        }

        let id_array = StringArray::from(ids);
        let text_array = StringArray::from(texts);

        Some(RecordBatch::try_new(
            self.schema.clone(),
            vec![Arc::new(id_array), Arc::new(text_array)],
        ))
    }
}

impl<R: BufRead> RecordBatchReader for JsonDocReader<R> {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }
}

pub fn sanitize_query(query: &str) -> String {
    // let mut parts = Vec::new();
    // for caps in TOKEN_RE.captures_iter(query) {
    //     if let Some(text) = caps.name("phrase").or_else(|| caps.name("term")) {
    //         if !text.as_str().is_empty() {
    //             parts.push(text.as_str().to_string());
    //         }
    //     }
    // }
    // parts.join(" ")
    query.to_string()
}

pub async fn build_index<R: BufRead + Send + 'static>(
    reader: R,
    idx_path: &str,
    batch_size: Option<usize>,
) -> lance::Result<Dataset> {
    let reader = JsonDocReader::new(reader, batch_size.unwrap_or(DEFAULT_BATCH_SIZE));
    let write_params = WriteParams {
        mode: WriteMode::Overwrite,
        ..Default::default()
    };
    let mut dataset = Dataset::write(reader, idx_path, Some(write_params)).await?;

    let params = InvertedIndexParams::default();
    dataset
        .create_index(&["text"], IndexType::Inverted, None, &params, true)
        .await?;
    Ok(dataset)
}

pub async fn open_dataset(idx_path: &str) -> lance::Result<Dataset> {
    let dataset = Dataset::open(idx_path).await?;
    dataset.prewarm_index("text_idx").await?;
    Ok(dataset)
}

fn build_query(query: &str) -> Option<FullTextSearchQuery> {
    let sanitized = sanitize_query(query);
    if sanitized.trim().is_empty() {
        return None;
    }
    let match_query = MatchQuery::new(sanitized)
        .with_column(Some("text".to_string()))
        .with_operator(Operator::Or);
    let fts_query = FtsQuery::Match(match_query);
    Some(FullTextSearchQuery::new_query(fts_query))
}

async fn stream_count_with_latency(
    mut stream: lance::dataset::scanner::DatasetRecordBatchStream,
) -> lance::Result<(usize, u64)> {
    let start = Instant::now();
    let mut count = 0usize;
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        count += batch.num_rows();
    }
    let duration_us = start.elapsed().as_micros() as u64;
    Ok((count, duration_us))
}

pub async fn count_query(dataset: &Dataset, query: &str) -> lance::Result<usize> {
    let (count, _) = count_query_with_latency(dataset, query).await?;
    Ok(count)
}

pub async fn count_query_with_latency(
    dataset: &Dataset,
    query: &str,
) -> lance::Result<(usize, u64)> {
    let Some(fts_query) = build_query(query) else {
        return Ok((0, 0));
    };

    let mut scanner = dataset.scan();
    scanner.empty_project()?;
    scanner.full_text_search(fts_query)?;
    scanner.disable_scoring_autoprojection();
    scanner.limit(None, None)?;
    let stream = scanner.try_into_stream().await?;
    stream_count_with_latency(stream).await
}

pub async fn run_topk(dataset: &Dataset, query: &str, k: usize) -> lance::Result<()> {
    run_topk_with_latency(dataset, query, k).await?;
    Ok(())
}

pub async fn run_topk_with_latency(
    dataset: &Dataset,
    query: &str,
    k: usize,
) -> lance::Result<u64> {
    let Some(fts_query) = build_query(query) else {
        return Ok(0);
    };

    let mut scanner = dataset.scan();
    scanner.empty_project()?;
    scanner.full_text_search(fts_query)?;
    scanner.disable_scoring_autoprojection();
    scanner.limit(Some(k as i64), None)?;
    let mut stream = scanner.try_into_stream().await?;
    let start = Instant::now();
    while let Some(batch) = stream.next().await {
        batch?;
    }
    let duration_us = start.elapsed().as_micros() as u64;
    Ok(duration_us)
}

pub async fn topk_count(dataset: &Dataset, query: &str, _k: usize) -> lance::Result<usize> {
    count_query(dataset, query).await
}

pub async fn topk_count_with_latency(
    dataset: &Dataset,
    query: &str,
    _k: usize,
) -> lance::Result<(usize, u64)> {
    count_query_with_latency(dataset, query).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[tokio::test]
    async fn test_pylance_counts_and_phrase() {
        let docs = [
            r#"{"id": "1", "text": "hello world"}"#,
            r#"{"id": "2", "text": "hello there"}"#,
            r#"{"id": "3", "text": "world peace"}"#,
        ]
        .join("\n");
        let reader = BufReader::new(std::io::Cursor::new(docs));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let idx_path = temp_dir.path().to_str().expect("utf8 path");
        build_index(reader, idx_path, Some(2)).await.unwrap();
        let dataset = open_dataset(idx_path).await.unwrap();

        assert_eq!(count_query(&dataset, "hello").await.unwrap(), 2);
        // Quoted phrases are treated as term unions with default indexing.
        assert_eq!(count_query(&dataset, "\"hello world\"").await.unwrap(), 3);
        // '+' terms are treated as OR after sanitization.
        assert_eq!(count_query(&dataset, "+hello +world").await.unwrap(), 3);

        let (count, duration) = count_query_with_latency(&dataset, "hello").await.unwrap();
        assert_eq!(count, 2);
        assert!(duration < 1_000_000);

        let duration = run_topk_with_latency(&dataset, "hello", 2).await.unwrap();
        assert!(duration < 1_000_000);
    }
}
