/*
 * @author t0bx
 * @since 23.10.2026
 */

pub(crate) const RELATION_SQL: &str = "SELECT c.oid, c.relkind::text, n.nspname::text, c.relname::text, \
            obj_description(c.oid, 'pg_class'), \
            CASE WHEN c.relkind = 'p' THEN pg_get_partkeydef(c.oid) END \
     FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE c.relname = $2::text AND n.nspname = COALESCE($1::text, current_schema()) \
       AND c.relkind IN ('r', 'p', 'f', 'v', 'm')";

macro_rules! columns_sql {
    ($extra_condition:literal) => {
        concat!(
            "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), \
                    NOT a.attnotnull, pg_get_expr(d.adbin, d.adrelid), a.attnum::int4, \
                    a.attidentity::text, a.attgenerated::text, \
                    col_description(a.attrelid, a.attnum) \
             FROM pg_catalog.pg_attribute a \
             LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped",
            $extra_condition,
            " ORDER BY a.attnum"
        )
    };
}

pub(crate) const COLUMNS_SQL: &str = columns_sql!("");

pub(crate) const VISIBLE_COLUMNS_SQL: &str = columns_sql!(" AND NOT a.attishidden");

pub(crate) const GENERATED_KEY_DEFAULTS: [&str; 2] = ["nextval(", "unique_rowid()"];

pub(crate) const INDEXES_SQL: &str = "SELECT i.relname::text, ix.indisunique, ix.indisprimary, \
            ARRAY(SELECT CASE WHEN k.attnum = 0 \
                              THEN pg_get_indexdef(ix.indexrelid, k.ord::int4, true) \
                              ELSE a.attname::text END \
                  FROM unnest(ix.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
                  LEFT JOIN pg_catalog.pg_attribute a \
                         ON a.attrelid = ix.indrelid AND a.attnum = k.attnum \
                  WHERE k.ord <= ix.indnkeyatts ORDER BY k.ord), \
            pg_get_indexdef(ix.indexrelid), \
            EXISTS (SELECT 1 FROM pg_catalog.pg_constraint con \
                    WHERE con.conindid = ix.indexrelid AND con.conrelid = ix.indrelid) \
     FROM pg_catalog.pg_index ix JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid \
     WHERE ix.indrelid = $1 \
     ORDER BY ix.indisprimary DESC, i.relname";

pub(crate) const FOREIGN_KEYS_SQL: &str = "SELECT con.conname::text, \
            ARRAY(SELECT a.attname::text \
                  FROM unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) \
                  JOIN pg_catalog.pg_attribute a \
                    ON a.attrelid = con.conrelid AND a.attnum = k.attnum \
                  ORDER BY k.ord), \
            rn.nspname::text, rc.relname::text, \
            ARRAY(SELECT a.attname::text \
                  FROM unnest(con.confkey) WITH ORDINALITY AS k(attnum, ord) \
                  JOIN pg_catalog.pg_attribute a \
                    ON a.attrelid = con.confrelid AND a.attnum = k.attnum \
                  ORDER BY k.ord) \
     FROM pg_catalog.pg_constraint con \
     JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid \
     JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace \
     WHERE con.conrelid = $1 AND con.contype = 'f' \
     ORDER BY con.conname";

pub(crate) const CONSTRAINTS_SQL: &str = "SELECT conname::text, pg_get_constraintdef(oid, true) \
     FROM pg_catalog.pg_constraint \
     WHERE conrelid = $1 AND contype IN ('p', 'u', 'c', 'f', 'x') \
     ORDER BY CASE contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'c' THEN 2 \
                           WHEN 'f' THEN 3 ELSE 4 END, conname";

pub(crate) const VIEW_QUERY_SQL: &str = "SELECT pg_get_viewdef($1::oid, true)";
