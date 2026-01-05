use std::io::BufRead;
use std::sync::Arc;

use arrow_array::{RecordBatch, RecordBatchReader, StringArray};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use futures::TryStreamExt;
use lance_index::scalar::inverted::query::{BooleanQuery, FtsQuery, MatchQuery, Occur, Operator};
use lance_index::scalar::FullTextSearchQuery;
use lancedb::database::CreateTableMode;
use lancedb::index::scalar::FtsIndexBuilder;
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::{connect, Table};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

const DEFAULT_BATCH_SIZE: usize = 10_000;
const NO_LIMIT: usize = i64::MAX as usize;

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

pub async fn build_index<R: BufRead + Send + 'static>(
    reader: R,
    idx_path: &str,
    batch_size: Option<usize>,
) -> lancedb::Result<Table> {
    let reader = JsonDocReader::new(reader, batch_size.unwrap_or(DEFAULT_BATCH_SIZE));
    let db = connect(idx_path).execute().await?;
    let table = db
        .create_table("docs", reader)
        .mode(CreateTableMode::Overwrite)
        .execute()
        .await?;

    table
        .create_index(&["text"], Index::FTS(FtsIndexBuilder::default()))
        .replace(true)
        .execute()
        .await?;

    Ok(table)
}

pub async fn open_table(idx_path: &str) -> lancedb::Result<Table> {
    let db = connect(idx_path).execute().await?;
    db.open_table("docs").execute().await
}

pub fn build_fts_query(query: &str, column: &str) -> FtsQuery {
    let mut terms: Vec<(Occur, FtsQuery)> = Vec::new();
    for caps in TOKEN_RE.captures_iter(query) {
        let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
        let text = caps
            .name("phrase")
            .or_else(|| caps.name("term"))
            .map(|m| m.as_str())
            .unwrap_or("");
        if text.is_empty() {
            continue;
        }

        let operator = if caps.name("phrase").is_some() {
            Operator::And
        } else {
            Operator::Or
        };
        let match_query = MatchQuery::new(text.to_string())
            .with_column(Some(column.to_string()))
            .with_operator(operator);

        let occur = match prefix {
            "+" => Occur::Must,
            "-" => Occur::MustNot,
            _ => Occur::Should,
        };
        terms.push((occur, FtsQuery::Match(match_query)));
    }

    if terms.is_empty() {
        return FtsQuery::Match(
            MatchQuery::new("".to_string()).with_column(Some(column.to_string())),
        );
    }
    if terms.len() == 1 && matches!(terms[0].0, Occur::Should) {
        return terms[0].1.clone();
    }
    FtsQuery::Boolean(BooleanQuery::new(terms))
}

fn query_builder(table: &Table, query: &str, limit: usize) -> lancedb::query::Query {
    let fts_query = build_fts_query(query, "text");
    table
        .query()
        .full_text_search(FullTextSearchQuery::new_query(fts_query))
        .select(Select::Columns(Vec::new()))
        .limit(limit)
}

pub async fn count_query(table: &Table, query: &str) -> lancedb::Result<usize> {
    let mut stream = query_builder(table, query, NO_LIMIT).execute().await?;
    let mut count = 0usize;
    while let Some(batch) = stream.try_next().await? {
        count += batch.num_rows();
    }
    Ok(count)
}

pub async fn run_topk(table: &Table, query: &str, k: usize) -> lancedb::Result<()> {
    let mut stream = query_builder(table, query, k).execute().await?;
    while let Some(_batch) = stream.try_next().await? {
        // drain
    }
    Ok(())
}

pub async fn topk_count(table: &Table, query: &str, _k: usize) -> lancedb::Result<usize> {
    count_query(table, query).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[tokio::test]
    async fn test_lancedb_counts_and_phrase() {
        let docs = vec![
            r#"{"id": "1", "text": "hello world"}"#,
            r#"{"id": "2", "text": "hello there"}"#,
            r#"{"id": "3", "text": "world peace"}"#,
        ]
        .join("\n");
        let reader = BufReader::new(std::io::Cursor::new(docs));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let idx_path = temp_dir.path().to_str().expect("utf8 path");
        build_index(reader, idx_path, Some(2)).await.unwrap();
        let table = open_table(idx_path).await.unwrap();

        assert_eq!(count_query(&table, "hello").await.unwrap(), 2);
        assert_eq!(count_query(&table, "\"hello world\"").await.unwrap(), 1);
        assert_eq!(count_query(&table, "+hello +world").await.unwrap(), 1);

        run_topk(&table, "hello", 2).await.unwrap();
    }
}
