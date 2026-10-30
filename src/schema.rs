// @generated automatically by Diesel CLI.

diesel::table! {
    funding_rates (symbol, time) {
        time -> Timestamptz,
        symbol -> Int4,
        rate -> Numeric,
        direction -> Text,
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
        begin_nonce -> Nullable<Int8>,
    }
}

diesel::table! {
    symbols (id) {
        id -> Int4,
        #[max_length = 255]
        name -> Varchar,
        lighter_market_id -> Int4,
        market_type -> Text,
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

diesel::joinable!(funding_rates -> symbols (symbol));
diesel::joinable!(orderbook_messages -> symbols (symbol));
diesel::joinable!(trades -> symbols (symbol));

diesel::allow_tables_to_appear_in_same_query!(funding_rates, orderbook_messages, symbols, trades,);
