-- Nonce that each delta continues from. src/ws.rs requires it to equal the
-- previous message's nonce (stored in `sequence`) for the same symbol in the
-- same connection, so a reader can detect a missing delta from the table alone.
-- Snapshots and rows written before this migration store NULL.
ALTER TABLE orderbook_messages ADD COLUMN begin_nonce BIGINT;
