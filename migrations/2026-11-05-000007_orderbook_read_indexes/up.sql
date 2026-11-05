-- Replay uses the ingestion ID as a cursor. Historical anchors read only snapshots.
DO $$
DECLARE
  spec record;
  existing_index regclass;
BEGIN
  FOR spec IN
    SELECT * FROM (VALUES
      ('orderbook_messages_symbol_id_idx',
       '(symbol, id)', ARRAY['symbol', 'id'], '0 0'::int2vector, NULL::text),
      ('orderbook_messages_snapshot_symbol_id_time_idx',
       '(symbol, id DESC, "time") WHERE message_type = ''snapshot''',
       ARRAY['symbol', 'id', 'time'], '0 3 0'::int2vector, '(message_type = ''snapshot''::text)')
    ) AS definitions(index_name, definition, columns, ordering, predicate)
  LOOP
    existing_index := to_regclass(format('public.%I', spec.index_name));
    IF existing_index IS NULL THEN
      EXECUTE format('CREATE INDEX %I ON public.orderbook_messages USING btree %s',
                     spec.index_name, spec.definition);
    ELSIF NOT EXISTS (
      SELECT 1
      FROM pg_index i
      JOIN pg_class c ON c.oid = i.indexrelid
      JOIN pg_am am ON am.oid = c.relam
      WHERE i.indexrelid = existing_index
        AND i.indrelid = 'public.orderbook_messages'::regclass
        AND i.indisvalid AND i.indisready AND i.indislive
        AND am.amname = 'btree'
        AND i.indnkeyatts = cardinality(spec.columns)
        AND i.indnatts = i.indnkeyatts AND i.indexprs IS NULL
        -- indoption stores DESC and NULLS FIRST as bits 1 and 2.
        AND i.indoption = spec.ordering
        AND pg_get_expr(i.indpred, i.indrelid) IS NOT DISTINCT FROM spec.predicate
        AND ARRAY(
          SELECT a.attname::text
          FROM generate_series(0, i.indnkeyatts - 1) AS key(position)
          JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[key.position]
          JOIN pg_opclass op ON op.oid = i.indclass[key.position]
          WHERE op.opcdefault AND a.attcollation = i.indcollation[key.position]
          ORDER BY key.position
        ) = spec.columns
    ) THEN
      RAISE EXCEPTION 'public.% exists but is invalid or incompatible', spec.index_name
        USING HINT = format('Expected a valid B-tree on public.orderbook_messages %s.', spec.definition);
    END IF;
  END LOOP;
END
$$;
