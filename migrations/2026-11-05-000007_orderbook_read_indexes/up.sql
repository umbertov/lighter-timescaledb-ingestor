-- Replay uses the ingestion ID as a cursor. Historical anchors read only snapshots.
DO $$
DECLARE
  spec record;
  has_usable_index boolean;
  has_name_conflict boolean;
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
    -- The catalog stores DESC and NULLS FIRST as bits 1 and 2 in indoption.
    SELECT coalesce(bool_or(usable), false),
           coalesce(bool_or(usable IS NOT TRUE) FILTER (WHERE relname = spec.index_name), false)
      INTO has_usable_index, has_name_conflict
      FROM (
        SELECT c.relname,
          i.indrelid = 'public.orderbook_messages'::regclass
          AND i.indisvalid AND i.indisready AND i.indislive
          AND am.amname = 'btree'
          AND i.indnkeyatts = cardinality(spec.columns)
          AND i.indnatts = i.indnkeyatts AND i.indexprs IS NULL
          AND i.indoption = spec.ordering
          AND pg_get_expr(i.indpred, i.indrelid) IS NOT DISTINCT FROM spec.predicate
          AND ARRAY(
            SELECT a.attname::text
            FROM generate_series(0, i.indnkeyatts - 1) AS key(position)
            JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[key.position]
            JOIN pg_opclass op ON op.oid = i.indclass[key.position]
            WHERE op.opcdefault AND a.attcollation = i.indcollation[key.position]
            ORDER BY key.position
          ) = spec.columns AS usable
        FROM pg_class c
        LEFT JOIN pg_index i ON i.indexrelid = c.oid
        LEFT JOIN pg_am am ON am.oid = c.relam
        WHERE c.relnamespace = 'public'::regnamespace
          AND (c.relname = spec.index_name OR i.indrelid = 'public.orderbook_messages'::regclass)
      ) AS candidates;

    IF has_name_conflict THEN
      RAISE EXCEPTION 'public.% exists but is invalid or incompatible', spec.index_name
        USING HINT = format('Expected a valid B-tree on public.orderbook_messages %s.', spec.definition);
    END IF;

    IF NOT has_usable_index THEN
      EXECUTE format('CREATE INDEX %I ON public.orderbook_messages USING btree %s',
                     spec.index_name, spec.definition);
    END IF;
  END LOOP;
END
$$;
