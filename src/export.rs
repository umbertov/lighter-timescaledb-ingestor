use arrow_array::builder::{BooleanBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch, TimestampMicrosecondArray};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use color_eyre::eyre::{eyre, Result, WrapErr};
use fallible_iterator::FallibleIterator;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use postgres::{Client, NoTls, Row};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::Arc;

const BATCH_SIZE: usize = 8_192;

/// Export selected datasets as Parquet files. An empty symbol list means all symbols.
pub fn export_parquet(
    database_url: &str,
    output_dir: &Path,
    symbols: &[String],
    trades: bool,
    orderbooks: bool,
) -> Result<()> {
    if !trades && !orderbooks {
        return Err(eyre!("select at least one dataset"));
    }
    let mut client = Client::connect(database_url, NoTls).wrap_err("connecting to PostgreSQL")?;
    let filter = symbol_filter(&mut client, symbols)?;
    fs::create_dir_all(output_dir).wrap_err("creating export directory")?;
    if trades {
        export_trades(&mut client, output_dir.join("trades.parquet"), &filter)?;
    }
    if orderbooks {
        export_orderbooks(
            &mut client,
            output_dir.join("orderbook_messages.parquet"),
            &filter,
        )?;
    }
    Ok(())
}

fn symbol_filter(client: &mut Client, symbols: &[String]) -> Result<String> {
    if symbols.is_empty() {
        return Ok(String::new());
    }
    let known: Vec<String> = client
        .query("SELECT name FROM symbols WHERE name = ANY($1)", &[&symbols])?
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    let unknown: Vec<&str> = symbols
        .iter()
        .map(String::as_str)
        .filter(|name| !known.iter().any(|value| value == name))
        .collect();
    if !unknown.is_empty() {
        return Err(eyre!("unknown symbol name(s): {}", unknown.join(", ")));
    }
    let names = symbols
        .iter()
        .map(|name| format!("'{}'", name.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(" WHERE s.name IN ({names})"))
}

fn parquet_writer(path: &Path, schema: Arc<Schema>) -> Result<ArrowWriter<std::fs::File>> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .wrap_err_with(|| format!("creating {}", path.display()))?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::ZSTD(Default::default()))
        .set_max_row_group_size(BATCH_SIZE)
        .build();
    ArrowWriter::try_new(file, schema, Some(properties)).wrap_err("creating Parquet writer")
}

fn export_trades(client: &mut Client, path: std::path::PathBuf, filter: &str) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "time",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("lighter_trade_id", DataType::Int64, false),
        Field::new("price", DataType::Float64, false),
        Field::new("size", DataType::Float64, false),
        Field::new("is_maker_ask", DataType::Boolean, false),
        Field::new("trade_type", DataType::Utf8, false),
    ]));
    let mut writer = parquet_writer(&path, schema.clone())?;
    let sql = format!("SELECT t.time, s.name, t.lighter_trade_id, t.price, t.size, t.is_maker_ask, t.trade_type FROM trades t JOIN symbols s ON s.id = t.symbol{filter} ORDER BY t.time, t.id");
    let mut rows = client.query_raw(&sql, std::iter::empty::<&str>())?;
    let mut batch = TradeBatch::new();
    while let Some(row) = rows.next()? {
        batch.push(&row);
        if batch.len() == BATCH_SIZE {
            batch.write(&mut writer, &schema)?;
        }
    }
    if !batch.is_empty() {
        batch.write(&mut writer, &schema)?;
    }
    writer.close().wrap_err("closing trades Parquet file")?;
    Ok(())
}

struct TradeBatch {
    times: Vec<i64>,
    symbols: StringBuilder,
    ids: Int64Builder,
    prices: Float64Builder,
    sizes: Float64Builder,
    makers: BooleanBuilder,
    kinds: StringBuilder,
}

impl TradeBatch {
    fn new() -> Self {
        Self {
            times: Vec::with_capacity(BATCH_SIZE),
            symbols: StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 8),
            ids: Int64Builder::with_capacity(BATCH_SIZE),
            prices: Float64Builder::with_capacity(BATCH_SIZE),
            sizes: Float64Builder::with_capacity(BATCH_SIZE),
            makers: BooleanBuilder::with_capacity(BATCH_SIZE),
            kinds: StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 12),
        }
    }

    fn len(&self) -> usize {
        self.times.len()
    }

    fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    fn push(&mut self, row: &Row) {
        self.times.push(
            row.get::<_, chrono::DateTime<chrono::Utc>>(0)
                .timestamp_micros(),
        );
        self.symbols.append_value(row.get::<_, String>(1));
        self.ids.append_value(row.get(2));
        self.prices.append_value(row.get(3));
        self.sizes.append_value(row.get(4));
        self.makers.append_value(row.get(5));
        self.kinds.append_value(row.get::<_, String>(6));
    }

    fn write(
        &mut self,
        writer: &mut ArrowWriter<std::fs::File>,
        schema: &Arc<Schema>,
    ) -> Result<()> {
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(std::mem::take(&mut self.times))
                        .with_timezone("UTC"),
                ) as ArrayRef,
                Arc::new(self.symbols.finish()),
                Arc::new(self.ids.finish()),
                Arc::new(self.prices.finish()),
                Arc::new(self.sizes.finish()),
                Arc::new(self.makers.finish()),
                Arc::new(self.kinds.finish()),
            ],
        )?;
        writer
            .write(&batch)
            .wrap_err("writing trades Parquet batch")
    }
}

fn export_orderbooks(client: &mut Client, path: std::path::PathBuf, filter: &str) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "time",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("message_type", DataType::Utf8, false),
        Field::new("sequence", DataType::Int64, true),
        Field::new("bids", DataType::Utf8, false),
        Field::new("asks", DataType::Utf8, false),
    ]));
    let mut writer = parquet_writer(&path, schema.clone())?;
    let sql = format!("SELECT o.time, s.name, o.message_type, o.sequence, o.bids, o.asks FROM orderbook_messages o JOIN symbols s ON s.id = o.symbol{filter} ORDER BY o.time, o.id");
    let mut rows = client.query_raw(&sql, std::iter::empty::<&str>())?;
    let mut batch = OrderbookBatch::new();
    while let Some(row) = rows.next()? {
        batch.push(&row)?;
        if batch.len() == BATCH_SIZE {
            batch.write(&mut writer, &schema)?;
        }
    }
    if !batch.is_empty() {
        batch.write(&mut writer, &schema)?;
    }
    writer.close().wrap_err("closing order-book Parquet file")?;
    Ok(())
}

struct OrderbookBatch {
    times: Vec<i64>,
    symbols: StringBuilder,
    types: StringBuilder,
    sequences: Int64Builder,
    bids: StringBuilder,
    asks: StringBuilder,
}

impl OrderbookBatch {
    fn new() -> Self {
        Self {
            times: Vec::with_capacity(BATCH_SIZE),
            symbols: StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 8),
            types: StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 8),
            sequences: Int64Builder::with_capacity(BATCH_SIZE),
            bids: StringBuilder::new(),
            asks: StringBuilder::new(),
        }
    }

    fn len(&self) -> usize {
        self.times.len()
    }

    fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    fn push(&mut self, row: &Row) -> Result<()> {
        self.times.push(
            row.get::<_, chrono::DateTime<chrono::Utc>>(0)
                .timestamp_micros(),
        );
        self.symbols.append_value(row.get::<_, String>(1));
        self.types.append_value(row.get::<_, String>(2));
        self.sequences.append_option(row.get(3));
        self.bids
            .append_value(serde_json::to_string(&row.get::<_, serde_json::Value>(4))?);
        self.asks
            .append_value(serde_json::to_string(&row.get::<_, serde_json::Value>(5))?);
        Ok(())
    }

    fn write(
        &mut self,
        writer: &mut ArrowWriter<std::fs::File>,
        schema: &Arc<Schema>,
    ) -> Result<()> {
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(std::mem::take(&mut self.times))
                        .with_timezone("UTC"),
                ) as ArrayRef,
                Arc::new(self.symbols.finish()),
                Arc::new(self.types.finish()),
                Arc::new(self.sequences.finish()),
                Arc::new(self.bids.finish()),
                Arc::new(self.asks.finish()),
            ],
        )?;
        writer
            .write(&batch)
            .wrap_err("writing order-book Parquet batch")
    }
}
