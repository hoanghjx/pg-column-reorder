import pg from 'pg'
import crypto from 'node:crypto'

const { Client } = pg
const GIB = 1024 ** 3
const REORDER_WARN_BYTES = GIB
const REORDER_BLOCK_BYTES = 5 * GIB

export const quoteIdent = (value) => `"${String(value).replaceAll('"', '""')}"`
const qualified = (schema, table) => `${quoteIdent(schema)}.${quoteIdent(table)}`
const literal = (value) => `'${String(value).replaceAll("'", "''")}'`
const formatBytes = (value) => {
  if (value < 1024) return `${value} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  const exponent = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length)
  return `${(value / 1024 ** exponent).toFixed(exponent > 2 ? 1 : 0)} ${units[exponent - 1]}`
}

export async function connect(connectionString) {
  const url = new URL(connectionString)
  if (process.env.DOCKERIZED === 'true' && ['localhost', '127.0.0.1'].includes(url.hostname)) url.hostname = 'host.docker.internal'
  const sslMode = url.searchParams.get('sslmode')
  const ssl = ['require', 'verify-ca', 'verify-full'].includes(sslMode)
    ? { rejectUnauthorized: false }
    : undefined
  if (ssl) url.searchParams.delete('sslmode')
  const client = new Client({ connectionString: url.toString(), application_name: 'pg-column-reorder', ssl })
  await client.connect()
  return client
}

export async function listTables(client) {
  const { rows } = await client.query(`
    SELECT n.nspname AS schema, c.relname AS table
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relkind IN ('r', 'p')
      AND n.nspname NOT IN ('pg_catalog', 'information_schema')
      AND n.nspname !~ '^pg_toast'
    ORDER BY n.nspname, c.relname
  `)
  return rows
}

export async function inspectTable(client, schema, table) {
  const relation = await client.query(`
    SELECT c.oid, n.oid AS namespace_oid, c.relkind, c.relpersistence, c.relrowsecurity, c.relforcerowsecurity,
           c.relreplident, c.reltablespace, c.reloptions,            c.reltuples::bigint::text AS estimated_rows,
           pg_table_size(c.oid)::text AS table_bytes,
           pg_indexes_size(c.oid)::text AS index_bytes,
           pg_total_relation_size(c.oid)::text AS total_bytes,
           am.amname AS access_method,
           pg_get_userbyid(c.relowner) AS owner,
           obj_description(c.oid, 'pg_class') AS comment,
           EXISTS (SELECT 1 FROM pg_inherits WHERE inhrelid = c.oid OR inhparent = c.oid) AS has_inheritance
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    LEFT JOIN pg_am am ON am.oid = c.relam
    WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')
  `, [schema, table])
  if (!relation.rowCount) throw new Error('Table not found')
  const rel = relation.rows[0]

  const columns = await client.query(`
    SELECT a.attname AS name, format_type(a.atttypid, a.atttypmod) AS type,
           a.attnotnull AS not_null, a.attidentity AS identity, a.attgenerated AS generated,
           pg_get_expr(d.adbin, d.adrelid) AS default_expr,
           col_description(a.attrelid, a.attnum) AS comment,
           a.attstorage AS storage, t.typstorage AS default_storage,
           a.attcompression AS compression, a.attstattarget::int4 AS statistics_target,
           a.attcollation, t.typcollation AS default_collation, a.attacl, a.attoptions,
           ps.last_value AS sequence_last_value, ps.start_value AS sequence_start_value,
           ps.min_value AS sequence_min_value, ps.max_value AS sequence_max_value,
           ps.increment_by AS sequence_increment_by, ps.cycle AS sequence_cycle,
           ps.cache_size AS sequence_cache_size,
           seq_ns.nspname AS sequence_schema, seq.relname AS sequence_name,
           seq.relacl AS sequence_acl, seq.relpersistence AS sequence_persistence,
           pg_get_userbyid(seq.relowner) AS sequence_owner,
           obj_description(seq.oid, 'pg_class') AS sequence_comment,
           sd.deptype AS sequence_dependency
    FROM pg_attribute a
    JOIN pg_type t ON t.oid = a.atttypid
    LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    LEFT JOIN pg_depend sd ON sd.refclassid = 'pg_class'::regclass
      AND sd.classid = 'pg_class'::regclass
      AND sd.refobjid = a.attrelid AND sd.refobjsubid = a.attnum
      AND sd.deptype IN ('i', 'a')
    LEFT JOIN pg_class seq ON seq.oid = sd.objid AND seq.relkind = 'S'
    LEFT JOIN pg_namespace seq_ns ON seq_ns.oid = seq.relnamespace
    LEFT JOIN pg_sequences ps ON ps.schemaname = seq_ns.nspname AND ps.sequencename = seq.relname
    WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped
    ORDER BY a.attnum
  `, [rel.oid])
  const constraints = await client.query(`
    SELECT con.conname AS name, con.contype AS type, pg_get_constraintdef(con.oid, true) AS definition,
           target_ns.nspname AS target_schema, target.relname AS target_table,
           CASE WHEN con.contype = 'n' THEN source_column.attname END AS column_name
    FROM pg_constraint con
    LEFT JOIN pg_class target ON target.oid = con.confrelid
    LEFT JOIN pg_namespace target_ns ON target_ns.oid = target.relnamespace
    LEFT JOIN pg_attribute source_column ON source_column.attrelid = con.conrelid
      AND source_column.attnum = con.conkey[1] AND con.contype = 'n'
    WHERE con.conrelid = $1 ORDER BY con.conname
  `, [rel.oid])
  const indexes = await client.query(`
    SELECT indexrelid::regclass::text AS name, pg_get_indexdef(indexrelid) AS definition
    FROM pg_index WHERE indrelid = $1 AND NOT indisprimary
      AND NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conindid = indexrelid)
    ORDER BY indexrelid::regclass::text
  `, [rel.oid])
  const dependencies = await client.query(`
    SELECT DISTINCT dependent_ns.nspname AS schema, dependent.relname AS name, dependent.relkind AS kind
    FROM pg_depend d
    JOIN pg_rewrite r ON r.oid = d.objid
    JOIN pg_class dependent ON dependent.oid = r.ev_class
    JOIN pg_namespace dependent_ns ON dependent_ns.oid = dependent.relnamespace
    WHERE d.refobjid = $1 AND dependent.oid <> $1
  `, [rel.oid])
  const triggers = await client.query('SELECT tgname AS name FROM pg_trigger WHERE tgrelid = $1 AND NOT tgisinternal', [rel.oid])
  const grants = await client.query(`
    SELECT CASE WHEN acl.grantee = 0 THEN 'PUBLIC' ELSE pg_get_userbyid(acl.grantee) END AS grantee,
           acl.privilege_type, acl.is_grantable
    FROM pg_class c
    CROSS JOIN LATERAL aclexplode(c.relacl) acl
    WHERE c.oid = $1
    ORDER BY grantee, privilege_type
  `, [rel.oid])
  const policies = await client.query('SELECT polname AS name FROM pg_policy WHERE polrelid = $1 ORDER BY polname', [rel.oid])
  const publications = await client.query(`
    SELECT DISTINCT p.pubname AS name
    FROM pg_publication p
    LEFT JOIN pg_publication_rel pr ON pr.prpubid = p.oid
    LEFT JOIN pg_publication_namespace pn ON pn.pnpubid = p.oid
    WHERE p.puballtables OR pr.prrelid = $1 OR pn.pnnspid = $2
    ORDER BY p.pubname
  `, [rel.oid, rel.namespace_oid])
  const statistics = await client.query('SELECT stxname AS name FROM pg_statistic_ext WHERE stxrelid = $1 ORDER BY stxname', [rel.oid])
  const incoming = await client.query(`
    SELECT source_ns.nspname AS schema, source.relname AS table, con.conname AS name,
           pg_get_constraintdef(con.oid, true) AS definition
    FROM pg_constraint con
    JOIN pg_class source ON source.oid = con.conrelid
    JOIN pg_namespace source_ns ON source_ns.oid = source.relnamespace
    WHERE con.confrelid = $1 AND con.contype = 'f'
    ORDER BY source_ns.nspname, source.relname, con.conname
  `, [rel.oid])
  const sequenceStates = await Promise.all(columns.rows.filter((column) => column.sequence_name).map(async (column) => {
    try {
      const state = await client.query(`SELECT is_called FROM ${qualified(column.sequence_schema, column.sequence_name)}`)
      return [column.name, state.rows[0]?.is_called ?? true]
    } catch {
      return [column.name, null]
    }
  }))
  const sequenceStateByColumn = new Map(sequenceStates)
  const tableBytes = Number(rel.table_bytes)
  const indexBytes = Number(rel.index_bytes)
  const totalBytes = Number(rel.total_bytes)
  const estimatedRequiredBytes = totalBytes * 2
  const preflightWarnings = []
  if (totalBytes >= REORDER_WARN_BYTES) {
    preflightWarnings.push(`Large table: ${formatBytes(totalBytes)} total; reserve at least ${formatBytes(estimatedRequiredBytes)} of free disk space`)
  }

  const blockers = []
  if (totalBytes >= REORDER_BLOCK_BYTES) blockers.push(`Table is ${formatBytes(totalBytes)}; automatic reorder is blocked at 5 GB`)

  if (rel.relkind === 'p' || rel.has_inheritance) blockers.push('Partitioned or inherited tables are not supported')
  if (rel.relrowsecurity || rel.relforcerowsecurity || policies.rowCount) blockers.push('Row-level security policies are not supported')
  if (rel.relreplident !== 'd') blockers.push('Custom replica identity is not supported')
  if (rel.reltablespace !== 0) blockers.push('Custom tablespace is not supported')
  if (rel.reloptions?.length) blockers.push('Table storage options are not supported')
  if (rel.access_method && rel.access_method !== 'heap') blockers.push(`Table access method is not supported: ${rel.access_method}`)
  if (publications.rowCount) blockers.push(`Publications: ${publications.rows.map((row) => row.name).join(', ')}`)
  if (statistics.rowCount) blockers.push(`Extended statistics: ${statistics.rows.map((row) => row.name).join(', ')}`)
  if (triggers.rowCount) blockers.push(`User triggers: ${triggers.rows.map((row) => row.name).join(', ')}`)
  if (dependencies.rowCount) blockers.push(`Dependent views/rules: ${dependencies.rows.map((row) => `${row.schema}.${row.name}`).join(', ')}`)
  for (const column of columns.rows) {
    column.sequence_is_called = sequenceStateByColumn.get(column.name) ?? null
    if (column.generated) blockers.push(`Generated column: ${column.name}`)
    if (!column.identity && column.default_expr?.startsWith('nextval(') && !column.sequence_name) blockers.push(`Unresolved sequence default: ${column.name}`)
    if (column.sequence_name && column.sequence_is_called === null) blockers.push(`Cannot inspect sequence state: ${column.name}`)
    if (column.storage !== column.default_storage) blockers.push(`Custom storage: ${column.name}`)
    if (column.compression) blockers.push(`Custom compression: ${column.name}`)
    if (column.statistics_target !== null && column.statistics_target !== -1) blockers.push(`Custom statistics target: ${column.name}`)
    if (column.attcollation !== column.default_collation) blockers.push(`Custom collation: ${column.name}`)
    if (column.attacl) blockers.push(`Column privileges: ${column.name}`)
    if (column.attoptions?.length) blockers.push(`Column options: ${column.name}`)
    if (column.sequence_name && column.sequence_owner !== rel.owner) blockers.push(`Custom sequence owner: ${column.name}`)
    if (column.sequence_acl) blockers.push(`Sequence privileges: ${column.name}`)
    if (column.sequence_comment !== null) blockers.push(`Sequence comment: ${column.name}`)
    if (column.sequence_persistence && column.sequence_persistence !== rel.relpersistence) blockers.push(`Sequence persistence: ${column.name}`)
  }

  return {
    schema,
    table,
    owner: rel.owner,
    comment: rel.comment,
    persistence: rel.relpersistence,
    estimatedRows: rel.estimated_rows,
    tableBytes,
    indexBytes,
    totalBytes,
    estimatedRequiredBytes,
    preflightWarnings,
    columns: columns.rows,
    constraints: constraints.rows,
    incomingForeignKeys: incoming.rows,
    indexes: indexes.rows,
    grants: grants.rows,
    blockers,
  }
}

function identityOptions(column) {
  return [
    `SEQUENCE NAME ${qualified(column.sequence_schema, column.sequence_name)}`,
    `START WITH ${column.sequence_start_value}`,
    `INCREMENT BY ${column.sequence_increment_by}`,
    `MINVALUE ${column.sequence_min_value}`,
    `MAXVALUE ${column.sequence_max_value}`,
    `CACHE ${column.sequence_cache_size}`,
    column.sequence_cycle ? 'CYCLE' : 'NO CYCLE',
  ].join(' ')
}

function columnSql(column, notNullConstraint) {
  let sql = `${quoteIdent(column.name)} ${column.type}`
  if (column.identity === 'a') sql += ` GENERATED ALWAYS AS IDENTITY (${identityOptions(column)})`
  else if (column.identity === 'd') sql += ` GENERATED BY DEFAULT AS IDENTITY (${identityOptions(column)})`
  else if (column.default_expr) sql += ` DEFAULT ${column.default_expr}`
  if (column.not_null) sql += notNullConstraint ? ` CONSTRAINT ${quoteIdent(notNullConstraint.name)} NOT NULL` : ' NOT NULL'
  return sql
}

export function buildPlan(metadata, order) {
  const existing = metadata.columns.map((column) => column.name)
  if (order.length !== existing.length || new Set(order).size !== order.length || order.some((name) => !existing.includes(name))) {
    throw new Error('Column order must contain every column exactly once')
  }
  if (metadata.blockers.length) throw new Error(metadata.blockers.join('\n'))

  const suffix = crypto.randomBytes(5).toString('hex')
  const tempTable = `__reorder_${suffix}`
  const backupTable = `__reorder_backup_${suffix}`
  const orderedColumns = order.map((name) => metadata.columns.find((column) => column.name === name))
  const source = qualified(metadata.schema, metadata.table)
  const temp = qualified(metadata.schema, tempTable)
  const backup = qualified(metadata.schema, backupTable)
  const names = orderedColumns.filter((column) => !column.generated).map((column) => quoteIdent(column.name)).join(', ')
  const outgoingForeignKeys = metadata.constraints.filter((item) => item.type === 'f')
  const incomingForeignKeys = metadata.incomingForeignKeys.filter((incoming) => !(
    incoming.schema === metadata.schema && incoming.table === metadata.table && outgoingForeignKeys.some((outgoing) => outgoing.name === incoming.name)
  ))
  const serialColumns = orderedColumns.filter((column) => !column.identity && column.sequence_name)
  const identityColumns = orderedColumns.filter((column) => column.identity && column.sequence_name)
  const identitySequenceBackups = new Map(identityColumns.map((column, index) => [column.name, `__reorder_sequence_${suffix}_${index}`]))
  const namedNotNullConstraints = metadata.constraints.filter((item) => item.type === 'n')
  const lockTables = new Set([source])
  for (const item of outgoingForeignKeys) lockTables.add(qualified(item.target_schema, item.target_table))
  for (const item of incomingForeignKeys) lockTables.add(qualified(item.schema, item.table))
  const statements = [
    'BEGIN;',
    `LOCK TABLE ${[...lockTables].sort().join(', ')} IN ACCESS EXCLUSIVE MODE;`,
    ...incomingForeignKeys.map((item) => `ALTER TABLE ${qualified(item.schema, item.table)} DROP CONSTRAINT ${quoteIdent(item.name)};`),
    ...serialColumns.map((column) => `ALTER SEQUENCE ${qualified(column.sequence_schema, column.sequence_name)} OWNED BY NONE;`),
    ...identityColumns.map((column) => `ALTER SEQUENCE ${qualified(column.sequence_schema, column.sequence_name)} RENAME TO ${quoteIdent(identitySequenceBackups.get(column.name))};`),
    `CREATE ${metadata.persistence === 'u' ? 'UNLOGGED ' : ''}TABLE ${temp} (\n  ${orderedColumns.map((column) => columnSql(column, namedNotNullConstraints.find((item) => item.column_name === column.name))).join(',\n  ')}\n);`,
    `INSERT INTO ${temp} (${names}) OVERRIDING SYSTEM VALUE SELECT ${names} FROM ${source};`,
    ...identityColumns.filter((column) => column.sequence_last_value !== null).map((column) => `SELECT setval(pg_get_serial_sequence(${literal(temp)}, ${literal(column.name)}), ${column.sequence_last_value}, ${column.sequence_is_called});`),
    `ALTER TABLE ${source} RENAME TO ${quoteIdent(backupTable)};`,
    `ALTER TABLE ${temp} RENAME TO ${quoteIdent(metadata.table)};`,
    `DROP TABLE ${backup};`,
    ...serialColumns.map((column) => `ALTER SEQUENCE ${qualified(column.sequence_schema, column.sequence_name)} OWNED BY ${source}.${quoteIdent(column.name)};`),
    ...metadata.constraints.filter((item) => !['f', 'n'].includes(item.type)).map((item) => `ALTER TABLE ${source} ADD CONSTRAINT ${quoteIdent(item.name)} ${item.definition};`),
    ...metadata.indexes.map((item) => `${item.definition};`),
    ...outgoingForeignKeys.map((item) => `ALTER TABLE ${source} ADD CONSTRAINT ${quoteIdent(item.name)} ${item.definition};`),
    ...incomingForeignKeys.map((item) => `ALTER TABLE ${qualified(item.schema, item.table)} ADD CONSTRAINT ${quoteIdent(item.name)} ${item.definition};`),
    `ALTER TABLE ${source} OWNER TO ${quoteIdent(metadata.owner)};`,
  ]
  if (metadata.comment !== null) statements.push(`COMMENT ON TABLE ${source} IS ${literal(metadata.comment)};`)
  for (const column of metadata.columns) {
    if (column.comment !== null) statements.push(`COMMENT ON COLUMN ${source}.${quoteIdent(column.name)} IS ${literal(column.comment)};`)
  }
  for (const grant of metadata.grants) {
    const grantee = grant.grantee === 'PUBLIC' ? 'PUBLIC' : quoteIdent(grant.grantee)
    statements.push(`GRANT ${grant.privilege_type} ON TABLE ${source} TO ${grantee}${grant.is_grantable ? ' WITH GRANT OPTION' : ''};`)
  }
  statements.push('COMMIT;')
  return { statements, sql: statements.join('\n') }
}

export async function renameColumn(client, schema, table, oldName, newName) {
  if (![schema, table, oldName, newName].every((value) => typeof value === 'string')) throw new Error('Invalid rename request')
  const target = newName.trim()
  if (!target) throw new Error('Column name is required')
  if (Buffer.byteLength(target) > 63) throw new Error('Column name must be at most 63 bytes')
  const metadata = await inspectTable(client, schema, table)
  if (!metadata.columns.some((column) => column.name === oldName)) throw new Error(`Column not found: ${oldName}`)
  if (oldName === target) return metadata
  if (metadata.columns.some((column) => column.name === target)) throw new Error(`Column already exists: ${target}`)
  await client.query(`ALTER TABLE ${qualified(schema, table)} RENAME COLUMN ${quoteIdent(oldName)} TO ${quoteIdent(target)}`)
  return inspectTable(client, schema, table)
}

export async function applyPlan(client, metadata, order) {
  const plan = buildPlan(metadata, order)
  try {
    for (const statement of plan.statements) await client.query(statement)
  } catch (error) {
    try {
      await client.query('ROLLBACK')
    } catch {
      throw error
    }
    throw error
  }
  return plan
}
