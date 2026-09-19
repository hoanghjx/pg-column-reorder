use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::State;
use tokio::sync::Mutex;
use tokio_postgres::{Client, Row};

struct DatabaseState(Mutex<Option<Client>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TableRef {
    schema: String,
    table: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ColumnMetadata {
    number: i16,
    name: String,
    #[serde(rename = "type")]
    data_type: String,
    not_null: bool,
    identity: String,
    generated: String,
    default_expr: Option<String>,
    comment: Option<String>,
    storage: String,
    default_storage: String,
    compression: Option<String>,
    statistics_target: Option<i32>,
    attcollation: u32,
    default_collation: u32,
    attacl: Option<Vec<String>>,
    attoptions: Option<Vec<String>>,
    sequence_last_value: Option<i64>,
    sequence_start_value: Option<i64>,
    sequence_min_value: Option<i64>,
    sequence_max_value: Option<i64>,
    sequence_increment_by: Option<i64>,
    sequence_cycle: Option<bool>,
    sequence_cache_size: Option<i64>,
    sequence_schema: Option<String>,
    sequence_name: Option<String>,
    sequence_acl: Option<Vec<String>>,
    sequence_persistence: Option<String>,
    sequence_owner: Option<String>,
    sequence_comment: Option<String>,
    sequence_dependency: Option<String>,
    sequence_is_called: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConstraintMetadata {
    name: String,
    #[serde(rename = "type")]
    constraint_type: String,
    definition: String,
    target_schema: Option<String>,
    target_table: Option<String>,
    column_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncomingForeignKey {
    schema: String,
    table: String,
    name: String,
    definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexMetadata {
    name: String,
    definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrantMetadata {
    grantee: String,
    privilege_type: String,
    is_grantable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TableMetadata {
    schema: String,
    table: String,
    owner: String,
    comment: Option<String>,
    persistence: String,
    record_count: String,
    columns: Vec<ColumnMetadata>,
    constraints: Vec<ConstraintMetadata>,
    incoming_foreign_keys: Vec<IncomingForeignKey>,
    indexes: Vec<IndexMetadata>,
    grants: Vec<GrantMetadata>,
    blockers: Vec<String>,
}

#[derive(Serialize)]
struct Plan {
    statements: Vec<String>,
    sql: String,
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(name))
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn row_string(row: &Row, name: &str) -> String {
    row.get::<_, String>(name)
}

async fn list_tables(client: &Client) -> Result<Vec<TableRef>, String> {
    client
        .query(
            "SELECT n.nspname AS schema, c.relname AS table
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE c.relkind IN ('r', 'p')
               AND n.nspname NOT IN ('pg_catalog', 'information_schema')
               AND n.nspname !~ '^pg_toast'
             ORDER BY n.nspname, c.relname",
            &[],
        )
        .await
        .map_err(|error| error.to_string())
        .map(|rows| {
            rows.into_iter()
                .map(|row| TableRef {
                    schema: row_string(&row, "schema"),
                    table: row_string(&row, "table"),
                })
                .collect()
        })
}

async fn inspect_table(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<TableMetadata, String> {
    let relation = client
        .query_opt(
            "SELECT c.oid, n.oid AS namespace_oid, c.relkind::text, c.relpersistence::text,
                    c.relrowsecurity, c.relforcerowsecurity, c.relreplident::text, c.reltablespace,
                    c.reloptions, am.amname AS access_method, pg_get_userbyid(c.relowner) AS owner,
                    obj_description(c.oid, 'pg_class') AS comment,
                    EXISTS (SELECT 1 FROM pg_inherits WHERE inhrelid = c.oid OR inhparent = c.oid) AS has_inheritance
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             LEFT JOIN pg_am am ON am.oid = c.relam
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')",
            &[&schema, &table],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Table not found".to_string())?;
    let oid: u32 = relation.get("oid");
    let namespace_oid: u32 = relation.get("namespace_oid");

    let column_rows = client
        .query(
            "SELECT a.attnum AS number, a.attname AS name, format_type(a.atttypid, a.atttypmod) AS type,
                    a.attnotnull AS not_null, a.attidentity::text AS identity, a.attgenerated::text AS generated,
                    pg_get_expr(d.adbin, d.adrelid) AS default_expr, col_description(a.attrelid, a.attnum) AS comment,
                    a.attstorage::text AS storage, t.typstorage::text AS default_storage,
                    NULLIF(a.attcompression::text, '') AS compression, a.attstattarget::int4 AS statistics_target,
                    a.attcollation, t.typcollation AS default_collation, a.attacl::text[], a.attoptions,
                    ps.last_value, ps.start_value, ps.min_value, ps.max_value, ps.increment_by, ps.cycle,
                    ps.cache_size, seq_ns.nspname AS sequence_schema, seq.relname AS sequence_name,
                    seq.relacl::text[] AS sequence_acl, seq.relpersistence::text AS sequence_persistence,
                    pg_get_userbyid(seq.relowner) AS sequence_owner, obj_description(seq.oid, 'pg_class') AS sequence_comment,
                    sd.deptype::text AS sequence_dependency
             FROM pg_attribute a
             JOIN pg_type t ON t.oid = a.atttypid
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             LEFT JOIN LATERAL (
               SELECT depend.objid, depend.deptype
               FROM pg_depend depend
               JOIN pg_class candidate ON candidate.oid = depend.objid AND candidate.relkind = 'S'
               WHERE depend.refclassid = 'pg_class'::regclass
                 AND depend.classid = 'pg_class'::regclass AND depend.refobjid = a.attrelid
                 AND depend.refobjsubid = a.attnum AND depend.deptype IN ('i', 'a')
               ORDER BY depend.deptype
               LIMIT 1
             ) sd ON true
             LEFT JOIN pg_class seq ON seq.oid = sd.objid
             LEFT JOIN pg_namespace seq_ns ON seq_ns.oid = seq.relnamespace
             LEFT JOIN pg_sequences ps ON ps.schemaname = seq_ns.nspname AND ps.sequencename = seq.relname
             WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped
             ORDER BY a.attnum",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?;

    let mut columns = Vec::with_capacity(column_rows.len());
    for row in column_rows {
        let sequence_schema: Option<String> = row.get("sequence_schema");
        let sequence_name: Option<String> = row.get("sequence_name");
        let sequence_is_called =
            if let (Some(seq_schema), Some(seq_name)) = (&sequence_schema, &sequence_name) {
                client
                    .query_one(
                        &format!("SELECT is_called FROM {}", qualified(seq_schema, seq_name)),
                        &[],
                    )
                    .await
                    .ok()
                    .map(|state| state.get::<_, bool>("is_called"))
            } else {
                None
            };
        columns.push(ColumnMetadata {
            number: row.get("number"),
            name: row.get("name"),
            data_type: row.get("type"),
            not_null: row.get("not_null"),
            identity: row.get("identity"),
            generated: row.get("generated"),
            default_expr: row.get("default_expr"),
            comment: row.get("comment"),
            storage: row.get("storage"),
            default_storage: row.get("default_storage"),
            compression: row.get("compression"),
            statistics_target: row.get("statistics_target"),
            attcollation: row.get("attcollation"),
            default_collation: row.get("default_collation"),
            attacl: row.get("attacl"),
            attoptions: row.get("attoptions"),
            sequence_last_value: row.get("last_value"),
            sequence_start_value: row.get("start_value"),
            sequence_min_value: row.get("min_value"),
            sequence_max_value: row.get("max_value"),
            sequence_increment_by: row.get("increment_by"),
            sequence_cycle: row.get("cycle"),
            sequence_cache_size: row.get("cache_size"),
            sequence_schema,
            sequence_name,
            sequence_acl: row.get("sequence_acl"),
            sequence_persistence: row.get("sequence_persistence"),
            sequence_owner: row.get("sequence_owner"),
            sequence_comment: row.get("sequence_comment"),
            sequence_dependency: row.get("sequence_dependency"),
            sequence_is_called,
        });
    }

    let constraints = client
        .query(
            "SELECT con.conname AS name, con.contype::text AS type, pg_get_constraintdef(con.oid, true) AS definition,
                    target_ns.nspname AS target_schema, target.relname AS target_table,
                    CASE WHEN con.contype = 'n' THEN source_column.attname END AS column_name
             FROM pg_constraint con
             LEFT JOIN pg_class target ON target.oid = con.confrelid
             LEFT JOIN pg_namespace target_ns ON target_ns.oid = target.relnamespace
             LEFT JOIN pg_attribute source_column ON source_column.attrelid = con.conrelid
               AND source_column.attnum = con.conkey[1] AND con.contype = 'n'
             WHERE con.conrelid = $1 ORDER BY con.conname",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| ConstraintMetadata {
            name: row.get("name"),
            constraint_type: row.get("type"),
            definition: row.get("definition"),
            target_schema: row.get("target_schema"),
            target_table: row.get("target_table"),
            column_name: row.get("column_name"),
        })
        .collect::<Vec<_>>();

    let indexes = client
        .query(
            "SELECT indexrelid::regclass::text AS name, pg_get_indexdef(indexrelid) AS definition
             FROM pg_index WHERE indrelid = $1 AND NOT indisprimary
               AND NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conindid = indexrelid)
             ORDER BY indexrelid::regclass::text",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| IndexMetadata {
            name: row.get("name"),
            definition: row.get("definition"),
        })
        .collect::<Vec<_>>();

    let incoming_foreign_keys = client
        .query(
            "SELECT source_ns.nspname AS schema, source.relname AS table, con.conname AS name,
                    pg_get_constraintdef(con.oid, true) AS definition
             FROM pg_constraint con
             JOIN pg_class source ON source.oid = con.conrelid
             JOIN pg_namespace source_ns ON source_ns.oid = source.relnamespace
             WHERE con.confrelid = $1 AND con.contype = 'f'
             ORDER BY source_ns.nspname, source.relname, con.conname",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| IncomingForeignKey {
            schema: row.get("schema"),
            table: row.get("table"),
            name: row.get("name"),
            definition: row.get("definition"),
        })
        .collect::<Vec<_>>();

    let grants = client
        .query(
            "SELECT CASE WHEN acl.grantee = 0 THEN 'PUBLIC' ELSE pg_get_userbyid(acl.grantee) END AS grantee,
                    acl.privilege_type, acl.is_grantable
             FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) acl
             WHERE c.oid = $1 ORDER BY grantee, privilege_type",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| GrantMetadata {
            grantee: row.get("grantee"),
            privilege_type: row.get("privilege_type"),
            is_grantable: row.get("is_grantable"),
        })
        .collect::<Vec<_>>();

    let dependency_rows = client
        .query(
            "SELECT DISTINCT dependent_ns.nspname AS schema, dependent.relname AS name
         FROM pg_depend d JOIN pg_rewrite r ON r.oid = d.objid
         JOIN pg_class dependent ON dependent.oid = r.ev_class
         JOIN pg_namespace dependent_ns ON dependent_ns.oid = dependent.relnamespace
         WHERE d.refobjid = $1 AND dependent.oid <> $1",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?;
    let trigger_rows = client
        .query(
            "SELECT tgname AS name FROM pg_trigger WHERE tgrelid = $1 AND NOT tgisinternal",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?;
    let policy_rows = client
        .query(
            "SELECT polname AS name FROM pg_policy WHERE polrelid = $1 ORDER BY polname",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?;
    let publication_rows = client
        .query(
            "SELECT DISTINCT p.pubname AS name FROM pg_publication p
         LEFT JOIN pg_publication_rel pr ON pr.prpubid = p.oid
         LEFT JOIN pg_publication_namespace pn ON pn.pnpubid = p.oid
         WHERE p.puballtables OR pr.prrelid = $1 OR pn.pnnspid = $2 ORDER BY p.pubname",
            &[&oid, &namespace_oid],
        )
        .await
        .map_err(|error| error.to_string())?;
    let statistic_rows = client
        .query(
            "SELECT stxname AS name FROM pg_statistic_ext WHERE stxrelid = $1 ORDER BY stxname",
            &[&oid],
        )
        .await
        .map_err(|error| error.to_string())?;
    let record_count: String = client
        .query_one(
            &format!(
                "SELECT count(*)::text AS count FROM {}",
                qualified(schema, table)
            ),
            &[],
        )
        .await
        .map_err(|error| error.to_string())?
        .get("count");

    let owner: String = relation.get("owner");
    let persistence: String = relation.get("relpersistence");
    let mut blockers = Vec::new();
    if relation.get::<_, String>("relkind") == "p" || relation.get::<_, bool>("has_inheritance") {
        blockers.push("Partitioned or inherited tables are not supported".into());
    }
    if relation.get::<_, bool>("relrowsecurity")
        || relation.get::<_, bool>("relforcerowsecurity")
        || !policy_rows.is_empty()
    {
        blockers.push("Row-level security policies are not supported".into());
    }
    if relation.get::<_, String>("relreplident") != "d" {
        blockers.push("Custom replica identity is not supported".into());
    }
    if relation.get::<_, u32>("reltablespace") != 0 {
        blockers.push("Custom tablespace is not supported".into());
    }
    if relation
        .get::<_, Option<Vec<String>>>("reloptions")
        .is_some_and(|value| !value.is_empty())
    {
        blockers.push("Table storage options are not supported".into());
    }
    if relation
        .get::<_, Option<String>>("access_method")
        .is_some_and(|value| value != "heap")
    {
        blockers.push("Custom table access method is not supported".into());
    }
    push_named_blocker(&mut blockers, "Publications", &publication_rows);
    push_named_blocker(&mut blockers, "Extended statistics", &statistic_rows);
    push_named_blocker(&mut blockers, "User triggers", &trigger_rows);
    if !dependency_rows.is_empty() {
        blockers.push(format!(
            "Dependent views/rules: {}",
            dependency_rows
                .iter()
                .map(|row| format!(
                    "{}.{}",
                    row.get::<_, String>("schema"),
                    row.get::<_, String>("name")
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for column in &columns {
        if !column.generated.is_empty() {
            blockers.push(format!("Generated column: {}", column.name));
        }
        if column.identity.is_empty()
            && column
                .default_expr
                .as_deref()
                .is_some_and(|value| value.starts_with("nextval("))
            && column.sequence_name.is_none()
        {
            blockers.push(format!("Unresolved sequence default: {}", column.name));
        }
        if column.sequence_name.is_some() && column.sequence_is_called.is_none() {
            blockers.push(format!("Cannot inspect sequence state: {}", column.name));
        }
        if column.storage != column.default_storage {
            blockers.push(format!("Custom storage: {}", column.name));
        }
        if column.compression.is_some() {
            blockers.push(format!("Custom compression: {}", column.name));
        }
        if column.statistics_target.is_some_and(|value| value != -1) {
            blockers.push(format!("Custom statistics target: {}", column.name));
        }
        if column.attcollation != column.default_collation {
            blockers.push(format!("Custom collation: {}", column.name));
        }
        if column.attacl.is_some() {
            blockers.push(format!("Column privileges: {}", column.name));
        }
        if column
            .attoptions
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        {
            blockers.push(format!("Column options: {}", column.name));
        }
        if column.sequence_name.is_some()
            && column.sequence_owner.as_deref() != Some(owner.as_str())
        {
            blockers.push(format!("Custom sequence owner: {}", column.name));
        }
        if column.sequence_acl.is_some() {
            blockers.push(format!("Sequence privileges: {}", column.name));
        }
        if column.sequence_comment.is_some() {
            blockers.push(format!("Sequence comment: {}", column.name));
        }
        if column
            .sequence_persistence
            .as_deref()
            .is_some_and(|value| value != persistence)
        {
            blockers.push(format!("Sequence persistence: {}", column.name));
        }
    }

    Ok(TableMetadata {
        schema: schema.into(),
        table: table.into(),
        owner,
        comment: relation.get("comment"),
        persistence,
        record_count,
        columns,
        constraints,
        incoming_foreign_keys,
        indexes,
        grants,
        blockers,
    })
}

fn push_named_blocker(blockers: &mut Vec<String>, label: &str, rows: &[Row]) {
    if !rows.is_empty() {
        blockers.push(format!(
            "{}: {}",
            label,
            rows.iter()
                .map(|row| row.get::<_, String>("name"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn identity_options(column: &ColumnMetadata) -> Result<String, String> {
    Ok(format!(
        "SEQUENCE NAME {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} CACHE {} {}",
        qualified(
            required(&column.sequence_schema, "sequence schema")?,
            required(&column.sequence_name, "sequence name")?
        ),
        required(&column.sequence_start_value, "sequence start")?,
        required(&column.sequence_increment_by, "sequence increment")?,
        required(&column.sequence_min_value, "sequence min")?,
        required(&column.sequence_max_value, "sequence max")?,
        required(&column.sequence_cache_size, "sequence cache")?,
        if column.sequence_cycle.unwrap_or(false) {
            "CYCLE"
        } else {
            "NO CYCLE"
        },
    ))
}

fn required<'a, T>(value: &'a Option<T>, label: &str) -> Result<&'a T, String> {
    value.as_ref().ok_or_else(|| format!("Missing {}", label))
}

fn column_sql(
    column: &ColumnMetadata,
    not_null_constraint: Option<&ConstraintMetadata>,
) -> Result<String, String> {
    let mut sql = format!("{} {}", quote_ident(&column.name), column.data_type);
    if column.identity == "a" {
        sql.push_str(&format!(
            " GENERATED ALWAYS AS IDENTITY ({})",
            identity_options(column)?
        ));
    } else if column.identity == "d" {
        sql.push_str(&format!(
            " GENERATED BY DEFAULT AS IDENTITY ({})",
            identity_options(column)?
        ));
    } else if let Some(default_expr) = &column.default_expr {
        sql.push_str(&format!(" DEFAULT {}", default_expr));
    }
    if column.not_null {
        if let Some(constraint) = not_null_constraint {
            sql.push_str(&format!(
                " CONSTRAINT {} NOT NULL",
                quote_ident(&constraint.name)
            ));
        } else {
            sql.push_str(" NOT NULL");
        }
    }
    Ok(sql)
}

fn build_plan(metadata: &TableMetadata, order: &[String]) -> Result<Plan, String> {
    let existing = metadata
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let unique = order.iter().collect::<HashSet<_>>();
    if order.len() != existing.len()
        || unique.len() != order.len()
        || order.iter().any(|name| !existing.contains(name))
    {
        return Err("Column order must contain every column exactly once".into());
    }
    if !metadata.blockers.is_empty() {
        return Err(metadata.blockers.join("\n"));
    }

    let mut bytes = [0_u8; 5];
    rand::rng().fill_bytes(&mut bytes);
    let suffix = hex::encode(bytes);
    let temp_table = format!("__reorder_{}", suffix);
    let backup_table = format!("__reorder_backup_{}", suffix);
    let by_name = metadata
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect::<HashMap<_, _>>();
    let ordered = order
        .iter()
        .map(|name| {
            by_name
                .get(name.as_str())
                .copied()
                .ok_or_else(|| format!("Column not found: {}", name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source = qualified(&metadata.schema, &metadata.table);
    let temp = qualified(&metadata.schema, &temp_table);
    let backup = qualified(&metadata.schema, &backup_table);
    let names = ordered
        .iter()
        .filter(|column| column.generated.is_empty())
        .map(|column| quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let outgoing = metadata
        .constraints
        .iter()
        .filter(|item| item.constraint_type == "f")
        .collect::<Vec<_>>();
    let incoming = metadata
        .incoming_foreign_keys
        .iter()
        .filter(|item| {
            !(item.schema == metadata.schema
                && item.table == metadata.table
                && outgoing.iter().any(|outgoing| outgoing.name == item.name))
        })
        .collect::<Vec<_>>();
    let serial = ordered
        .iter()
        .copied()
        .filter(|column| column.identity.is_empty() && column.sequence_name.is_some())
        .collect::<Vec<_>>();
    let identities = ordered
        .iter()
        .copied()
        .filter(|column| !column.identity.is_empty() && column.sequence_name.is_some())
        .collect::<Vec<_>>();
    let named_not_null = metadata
        .constraints
        .iter()
        .filter(|item| item.constraint_type == "n")
        .collect::<Vec<_>>();
    let mut locks = HashSet::from([source.clone()]);
    for item in &outgoing {
        if let (Some(schema), Some(table)) = (&item.target_schema, &item.target_table) {
            locks.insert(qualified(schema, table));
        }
    }
    for item in &incoming {
        locks.insert(qualified(&item.schema, &item.table));
    }
    let mut locks = locks.into_iter().collect::<Vec<_>>();
    locks.sort();

    let mut statements = vec![
        "BEGIN;".into(),
        format!("LOCK TABLE {} IN ACCESS EXCLUSIVE MODE;", locks.join(", ")),
    ];
    statements.extend(incoming.iter().map(|item| {
        format!(
            "ALTER TABLE {} DROP CONSTRAINT {};",
            qualified(&item.schema, &item.table),
            quote_ident(&item.name)
        )
    }));
    for column in &serial {
        statements.push(format!(
            "ALTER SEQUENCE {} OWNED BY NONE;",
            qualified(
                required(&column.sequence_schema, "sequence schema")?,
                required(&column.sequence_name, "sequence name")?
            )
        ));
    }
    for (index, column) in identities.iter().enumerate() {
        statements.push(format!(
            "ALTER SEQUENCE {} RENAME TO {};",
            qualified(
                required(&column.sequence_schema, "sequence schema")?,
                required(&column.sequence_name, "sequence name")?
            ),
            quote_ident(&format!("__reorder_sequence_{}_{}", suffix, index))
        ));
    }
    let definitions = ordered
        .iter()
        .map(|column| {
            column_sql(
                column,
                named_not_null
                    .iter()
                    .find(|constraint| {
                        constraint.column_name.as_deref() == Some(column.name.as_str())
                    })
                    .copied(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    statements.push(format!(
        "CREATE {}TABLE {} (\n  {}\n);",
        if metadata.persistence == "u" {
            "UNLOGGED "
        } else {
            ""
        },
        temp,
        definitions.join(",\n  ")
    ));
    statements.push(format!(
        "INSERT INTO {} ({}) OVERRIDING SYSTEM VALUE SELECT {} FROM {};",
        temp, names, names, source
    ));
    for column in identities
        .iter()
        .filter(|column| column.sequence_last_value.is_some())
    {
        statements.push(format!(
            "SELECT setval(pg_get_serial_sequence({}, {}), {}, {});",
            literal(&temp),
            literal(&column.name),
            column.sequence_last_value.unwrap_or_default(),
            column.sequence_is_called.unwrap_or(true)
        ));
    }
    statements.push(format!(
        "ALTER TABLE {} RENAME TO {};",
        source,
        quote_ident(&backup_table)
    ));
    statements.push(format!(
        "ALTER TABLE {} RENAME TO {};",
        temp,
        quote_ident(&metadata.table)
    ));
    statements.push(format!("DROP TABLE {};", backup));
    for column in &serial {
        statements.push(format!(
            "ALTER SEQUENCE {} OWNED BY {}.{};",
            qualified(
                required(&column.sequence_schema, "sequence schema")?,
                required(&column.sequence_name, "sequence name")?
            ),
            source,
            quote_ident(&column.name)
        ));
    }
    statements.extend(
        metadata
            .constraints
            .iter()
            .filter(|item| item.constraint_type != "f" && item.constraint_type != "n")
            .map(|item| {
                format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} {};",
                    source,
                    quote_ident(&item.name),
                    item.definition
                )
            }),
    );
    statements.extend(
        metadata
            .indexes
            .iter()
            .map(|item| format!("{};", item.definition)),
    );
    statements.extend(outgoing.iter().map(|item| {
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} {};",
            source,
            quote_ident(&item.name),
            item.definition
        )
    }));
    statements.extend(incoming.iter().map(|item| {
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} {};",
            qualified(&item.schema, &item.table),
            quote_ident(&item.name),
            item.definition
        )
    }));
    statements.push(format!(
        "ALTER TABLE {} OWNER TO {};",
        source,
        quote_ident(&metadata.owner)
    ));
    if let Some(comment) = &metadata.comment {
        statements.push(format!(
            "COMMENT ON TABLE {} IS {};",
            source,
            literal(comment)
        ));
    }
    for column in &metadata.columns {
        if let Some(comment) = &column.comment {
            statements.push(format!(
                "COMMENT ON COLUMN {}.{} IS {};",
                source,
                quote_ident(&column.name),
                literal(comment)
            ));
        }
    }
    for grant in &metadata.grants {
        statements.push(format!(
            "GRANT {} ON TABLE {} TO {}{};",
            grant.privilege_type,
            source,
            if grant.grantee == "PUBLIC" {
                "PUBLIC".into()
            } else {
                quote_ident(&grant.grantee)
            },
            if grant.is_grantable {
                " WITH GRANT OPTION"
            } else {
                ""
            }
        ));
    }
    statements.push("COMMIT;".into());
    Ok(Plan {
        sql: statements.join("\n"),
        statements,
    })
}

async fn with_client<T>(
    state: &State<'_, DatabaseState>,
    operation: impl for<'a> FnOnce(
        &'a Client,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, String>> + Send + 'a>,
    >,
) -> Result<T, String> {
    let guard = state.0.lock().await;
    let client = guard.as_ref().ok_or_else(|| "Not connected".to_string())?;
    operation(client).await
}

#[tauri::command]
async fn connect_database(
    connection_string: String,
    state: State<'_, DatabaseState>,
) -> Result<Vec<TableRef>, String> {
    let tls = TlsConnector::builder()
        .build()
        .map(MakeTlsConnector::new)
        .map_err(|error| error.to_string())?;
    let (client, connection) = tokio_postgres::connect(&connection_string, tls)
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn(async move {
        let _ = connection.await;
    });
    let tables = list_tables(&client).await?;
    *state.0.lock().await = Some(client);
    Ok(tables)
}

#[tauri::command]
async fn disconnect_database(state: State<'_, DatabaseState>) -> Result<(), String> {
    *state.0.lock().await = None;
    Ok(())
}

#[tauri::command]
async fn inspect_database_table(
    schema: String,
    table: String,
    state: State<'_, DatabaseState>,
) -> Result<TableMetadata, String> {
    with_client(&state, |client| {
        Box::pin(async move { inspect_table(client, &schema, &table).await })
    })
    .await
}

#[tauri::command]
async fn preview_reorder(
    schema: String,
    table: String,
    order: Vec<String>,
    state: State<'_, DatabaseState>,
) -> Result<Plan, String> {
    with_client(&state, |client| {
        Box::pin(async move { build_plan(&inspect_table(client, &schema, &table).await?, &order) })
    })
    .await
}

#[tauri::command]
async fn apply_reorder(
    schema: String,
    table: String,
    order: Vec<String>,
    state: State<'_, DatabaseState>,
) -> Result<TableMetadata, String> {
    with_client(&state, |client| {
        Box::pin(async move {
            let metadata = inspect_table(client, &schema, &table).await?;
            let plan = build_plan(&metadata, &order)?;
            for statement in &plan.statements {
                if let Err(error) = client.batch_execute(statement).await {
                    let _ = client.batch_execute("ROLLBACK").await;
                    return Err(error.to_string());
                }
            }
            inspect_table(client, &schema, &table).await
        })
    })
    .await
}

#[tauri::command]
async fn rename_column(
    schema: String,
    table: String,
    old_name: String,
    new_name: String,
    state: State<'_, DatabaseState>,
) -> Result<TableMetadata, String> {
    with_client(&state, |client| {
        Box::pin(async move {
            let target = new_name.trim();
            if target.is_empty() {
                return Err("Column name is required".into());
            }
            if target.len() > 63 {
                return Err("Column name must be at most 63 bytes".into());
            }
            let metadata = inspect_table(client, &schema, &table).await?;
            if !metadata
                .columns
                .iter()
                .any(|column| column.name == old_name)
            {
                return Err(format!("Column not found: {}", old_name));
            }
            if old_name == target {
                return Ok(metadata);
            }
            if metadata.columns.iter().any(|column| column.name == target) {
                return Err(format!("Column already exists: {}", target));
            }
            client
                .batch_execute(&format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {}",
                    qualified(&schema, &table),
                    quote_ident(&old_name),
                    quote_ident(target)
                ))
                .await
                .map_err(|error| error.to_string())?;
            inspect_table(client, &schema, &table).await
        })
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(DatabaseState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            connect_database,
            disconnect_database,
            inspect_database_table,
            preview_reorder,
            apply_reorder,
            rename_column
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_and_literals() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(literal("a'b"), "'a''b'");
    }

    #[test]
    #[ignore]
    fn inspects_all_database_tables() {
        let connection_string = std::env::var("PG_REORDER_TEST_DATABASE_URL").unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (client, connection) = tokio_postgres::connect(&connection_string, NoTls)
                    .await
                    .unwrap();
                tokio::spawn(async move { connection.await.unwrap() });
                for table in list_tables(&client).await.unwrap() {
                    inspect_table(&client, &table.schema, &table.table)
                        .await
                        .unwrap_or_else(|error| {
                            panic!("{}.{}: {error}", table.schema, table.table)
                        });
                }
            });
    }
}
