export interface TableRef {
  schema: string
  table: string
}

export interface ColumnMetadata {
  number: number
  name: string
  type: string
  not_null: boolean
  identity: string
  generated: string
  default_expr: string | null
  comment: string | null
  storage: string
  default_storage: string
  compression: string | null
  statistics_target: number | null
  attcollation: number
  default_collation: number
  attacl: string[] | null
  attoptions: string[] | null
  sequence_last_value: number | null
  sequence_start_value: number | null
  sequence_min_value: number | null
  sequence_max_value: number | null
  sequence_increment_by: number | null
  sequence_cycle: boolean | null
  sequence_cache_size: number | null
  sequence_schema: string | null
  sequence_name: string | null
  sequence_acl: string[] | null
  sequence_persistence: string | null
  sequence_owner: string | null
  sequence_comment: string | null
  sequence_dependency: string | null
  sequence_is_called: boolean | null
}

export interface ConstraintMetadata {
  name: string
  type: string
  definition: string
  target_schema: string | null
  target_table: string | null
  column_name: string | null
}

export interface IncomingForeignKey {
  schema: string
  table: string
  name: string
  definition: string
}

export interface IndexMetadata {
  name: string
  definition: string
}

export interface GrantMetadata {
  grantee: string
  privilege_type: string
  is_grantable: boolean
}

export interface TableMetadata {
  schema: string
  table: string
  owner: string
  comment: string | null
  persistence: string
  estimatedRows: string
  tableBytes: number
  indexBytes: number
  totalBytes: number
  estimatedRequiredBytes: number
  preflightWarnings: string[]
  columns: ColumnMetadata[]
  constraints: ConstraintMetadata[]
  incomingForeignKeys: IncomingForeignKey[]
  indexes: IndexMetadata[]
  grants: GrantMetadata[]
  blockers: string[]
}

export interface ConnectionProfile {
  label: string
  value: string
}

export interface Plan {
  statements: string[]
  sql: string
}
