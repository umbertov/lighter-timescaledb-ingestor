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
    let mut times = Vec::with_capacity(BATCH_SIZE);
    let mut symbols = StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 8);
    let mut ids = Int64Builder::with_capacity(BATCH_SIZE);
    let mut prices = Float64Builder::with_capacity(BATCH_SIZE);
    let mut sizes = Float64Builder::with_capacity(BATCH_SIZE);
    let mut makers = BooleanBuilder::with_capacity(BATCH_SIZE);
    let mut kinds = StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 12);
    while let Some(row) = rows.next()? {
        push_trade(
            &row,
            &mut times,
            &mut symbols,
            &mut ids,
            &mut prices,
            &mut sizes,
            &mut makers,
            &mut kinds,
        );
        if times.len() == BATCH_SIZE {
            write_trade_batch(
                &mut writer,
                &schema,
                &mut times,
                &mut symbols,
                &mut ids,
                &mut prices,
                &mut sizes,
                &mut makers,
                &mut kinds,
            )?;
        }
    }
    if !times.is_empty() {
        write_trade_batch(
            &mut writer,
            &schema,
            &mut times,
            &mut symbols,
            &mut ids,
            &mut prices,
            &mut sizes,
            &mut makers,
            &mut kinds,
        )?;
    }
    writer.close().wrap_err("closing trades Parquet file")?;
    Ok(())
}

fn push_trade(
    row: &Row,
    times: &mut Vec<i64>,
    symbols: &mut StringBuilder,
    ids: &mut Int64Builder,
    prices: &mut Float64Builder,
    sizes: &mut Float64Builder,
    makers: &mut BooleanBuilder,
    kinds: &mut StringBuilder,
) {
    times.push(
        row.get::<_, chrono::DateTime<chrono::Utc>>(0)
            .timestamp_micros(),
    );
    symbols.append_value(row.get::<_, String>(1));
    ids.append_value(row.get(2));
    prices.append_value(row.get(3));
    sizes.append_value(row.get(4));
    makers.append_value(row.get(5));
    kinds.append_value(row.get::<_, String>(6));
}

fn write_trade_batch(
    writer: &mut ArrowWriter<std::fs::File>,
    schema: &Arc<Schema>,
    times: &mut Vec<i64>,
    symbols: &mut StringBuilder,
    ids: &mut Int64Builder,
    prices: &mut Float64Builder,
    sizes: &mut Float64Builder,
    makers: &mut BooleanBuilder,
    kinds: &mut StringBuilder,
) -> Result<()> {
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(TimestampMicrosecondArray::from(std::mem::take(times)).with_timezone("UTC"))
                as ArrayRef,
            Arc::new(symbols.finish()),
            Arc::new(ids.finish()),
            Arc::new(prices.finish()),
            Arc::new(sizes.finish()),
            Arc::new(makers.finish()),
            Arc::new(kinds.finish()),
        ],
    )?;
    writer
        .write(&batch)
        .wrap_err("writing trades Parquet batch")
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
    let mut times = Vec::with_capacity(BATCH_SIZE);
    let mut symbols = StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 8);
    let mut types = StringBuilder::with_capacity(BATCH_SIZE, BATCH_SIZE * 8);
    let mut sequences = Int64Builder::with_capacity(BATCH_SIZE);
    let mut bids = StringBuilder::new();
    let mut asks = StringBuilder::new();
    while let Some(row) = rows.next()? {
        times.push(
            row.get::<_, chrono::DateTime<chrono::Utc>>(0)
                .timestamp_micros(),
        );
        symbols.append_value(row.get::<_, String>(1));
        types.append_value(row.get::<_, String>(2));
        sequences.append_option(row.get(3));
        bids.append_value(serde_json::to_string(&row.get::<_, serde_json::Value>(4))?);
        asks.append_value(serde_json::to_string(&row.get::<_, serde_json::Value>(5))?);
        if times.len() == BATCH_SIZE {
            write_orderbook_batch(
                &mut writer,
                &schema,
                &mut times,
                &mut symbols,
                &mut types,
                &mut sequences,
                &mut bids,
                &mut asks,
            )?;
        }
    }
    if !times.is_empty() {
        write_orderbook_batch(
            &mut writer,
            &schema,
            &mut times,
            &mut symbols,
            &mut types,
            &mut sequences,
            &mut bids,
            &mut asks,
        )?;
    }
    writer.close().wrap_err("closing order-book Parquet file")?;
    Ok(())
}

fn write_orderbook_batch(
    writer: &mut ArrowWriter<std::fs::File>,
    schema: &Arc<Schema>,
    times: &mut Vec<i64>,
    symbols: &mut StringBuilder,
    types: &mut StringBuilder,
    sequences: &mut Int64Builder,
    bids: &mut StringBuilder,
    asks: &mut StringBuilder,
) -> Result<()> {
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(TimestampMicrosecondArray::from(std::mem::take(times)).with_timezone("UTC"))
                as ArrayRef,
            Arc::new(symbols.finish()),
            Arc::new(types.finish()),
            Arc::new(sequences.finish()),
            Arc::new(bids.finish()),
            Arc::new(asks.finish()),
        ],
    )?;
    writer
        .write(&batch)
        .wrap_err("writing order-book Parquet batch")
}
