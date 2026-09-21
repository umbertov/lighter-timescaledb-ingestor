// Hand-written to match migrations/2026-09-21-000001_lighter_tables_v1 and
// migrations/2026-09-21-000002_trades_ws_and_dedup, in the shape
// `diesel print-schema` would generate. Regenerate for real
// (`diesel print-schema > src/schema.rs`, per diesel.toml) once a live
// database is available to run migrations against -- this file has NOT
// been verified against an actual migrated database.

diesel::table! {
    symbols (id) {
        id -> Int4,
        #[max_length = 255]
        name -> Varchar,
        lighter_market_id -> Int4,
    }
}

diesel::table! {
    trades (time, id) {
        id -> Int8,
        time -> Timestamptz,
        symbol -> Int4,
        lighter_trade_id -> Int8,
        price -> Float8,
        size -> Float8,
        is_maker_ask -> Bool,
        trade_type -> Text,
    }
}

diesel::table! {
    orderbook_messages (time, id) {
        id -> Int8,
        time -> Timestamptz,
        symbol -> Int4,
        message_type -> Text,
        sequence -> Nullable<Int8>,
        bids -> Jsonb,
        asks -> Jsonb,
    }
}

diesel::joinable!(trades -> symbols (symbol));
diesel::joinable!(orderbook_messages -> symbols (symbol));

diesel::allow_tables_to_appear_in_same_query!(orderbook_messages, symbols, trades,);
