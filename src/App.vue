<script setup lang="ts">
import { useDebounce, useStorage } from '@vueuse/core'
import { insertNodeAt, removeNode, useSortable } from '@vueuse/integrations/useSortable'
import { groupBy, keyBy, orderBy, uniqBy } from 'lodash-es'
import { AlertTriangle, Check, ChevronsUpDown, Database, Eye, EyeOff, LogOut, Search, Trash2 } from '@lucide/vue'
import { computed, nextTick, ref, useTemplateRef } from 'vue'
import Alert from '@/components/ui/Alert.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import { api, setSessionToken } from '@/lib/api'
import type { ColumnMetadata, ConnectionProfile, Plan, TableMetadata, TableRef } from '@/types/database'

const leadingNames = ['id', 'uuid', 'guid', 'pk', 'key']
const trailingNames = ['created_at', 'updated_at', 'deleted_at']
const history = useStorage<ConnectionProfile[]>('pg-column-reorder-connections', [])
const connectionString = ref('')
const showConnection = ref(false)
const activeConnection = ref('')
const tables = ref<TableRef[]>([])
const current = ref<TableMetadata | null>(null)
const columns = ref<ColumnMetadata[]>([])
const originalOrder = ref<string[]>([])
const selected = ref<string[]>([])
const tableSearch = ref('')
const debouncedSearch = useDebounce(tableSearch, 150)
const busy = ref(false)
const message = ref('')
const error = ref('')
const success = ref('')
const editing = ref<string | null>(null)
const editName = ref('')
const plan = ref<Plan | null>(null)
const columnsElement = useTemplateRef<HTMLElement>('columnsElement')
let dragSnapshot: ColumnMetadata[] = []

const { start: startSortable, stop: stopSortable } = useSortable(columnsElement, columns, {
  handle: '.drag-handle',
  animation: 120,
  ghostClass: 'sortable-ghost',
  dragClass: 'sortable-drag',
  onStart(event) {
    dragSnapshot = [...columns.value]
    const name = dragSnapshot[event.oldIndex ?? -1]?.name
    if (name && !selected.value.includes(name)) selected.value = [name]
  },
  onUpdate(event) {
    if (event.oldIndex === undefined || event.newIndex === undefined) return
    removeNode(event.item)
    insertNodeAt(event.from, event.item, event.oldIndex)
    const moved = dragSnapshot[event.oldIndex]
    const movingNames = selected.value.includes(moved.name) ? selected.value : [moved.name]
    const moving = dragSnapshot.filter((column) => movingNames.includes(column.name))
    const rest = dragSnapshot.filter((column) => !movingNames.includes(column.name))
    const selectedBefore = dragSnapshot.slice(0, event.newIndex).filter((column) => movingNames.includes(column.name)).length
    const target = Math.max(0, Math.min(event.newIndex - selectedBefore, rest.length))
    nextTick(() => columns.value = [...rest.slice(0, target), ...moving, ...rest.slice(target)])
  },
})

const connected = computed(() => Boolean(activeConnection.value))
const filteredTables = computed(() => {
  const needle = debouncedSearch.value.trim().toLowerCase()
  return needle ? tables.value.filter((item) => `${item.schema}.${item.table}`.toLowerCase().includes(needle)) : tables.value
})
const groupedTables = computed(() => groupBy(orderBy(filteredTables.value, ['schema', 'table']), 'schema'))
const columnKey = (column: ColumnMetadata) => `${column.number}:${column.name}`
const currentOrder = computed(() => columns.value.map((column) => column.name))
const changed = computed(() => currentOrder.value.some((name, index) => name !== originalOrder.value[index]))
const canApply = computed(() => Boolean(current.value && !current.value.blockers.length && changed.value && !busy.value))
const formatBytes = (value: number) => {
  if (value < 1024) return `${value} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  const exponent = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length)
  return `${(value / 1024 ** exponent).toFixed(exponent > 2 ? 1 : 0)} ${units[exponent - 1]}`
}
const metrics = computed(() => current.value ? [
  ['Rows (estimate)', Number(current.value.estimatedRows) < 0 ? 'Unknown' : Number(current.value.estimatedRows).toLocaleString()],
  ['Table', formatBytes(current.value.tableBytes)],
  ['Indexes', formatBytes(current.value.indexBytes)],
  ['Total', formatBytes(current.value.totalBytes)],
  ['Disk needed', formatBytes(current.value.estimatedRequiredBytes)],
  ['Status', current.value.blockers.length ? 'Blocked' : current.value.preflightWarnings.length ? 'Warning' : 'Ready'],
] : [])

function clearNotices() {
  message.value = ''
  error.value = ''
  success.value = ''
  plan.value = null
}

function errorText(value: unknown) {
  return value instanceof Error ? value.message : String(value)
}

function connectionProfile(value: string): ConnectionProfile {
  const url = new URL(value)
  if (!['postgres:', 'postgresql:'].includes(url.protocol)) throw new Error('Connection string phải dùng postgres:// hoặc postgresql://')
  return {
    label: `${decodeURIComponent(url.username) || 'postgres'}@${url.hostname}:${url.port || '5432'}/${url.pathname.slice(1)}`,
    value,
  }
}

function saveHistory(value: string) {
  history.value = uniqBy([connectionProfile(value), ...history.value], 'value').slice(0, 10)
}

function suggestedColumns(items: ColumnMetadata[]) {
  const leading = items.filter((column) => leadingNames.includes(column.name))
  const trailing = items.filter((column) => trailingNames.includes(column.name))
  const middle = items.filter((column) => !leadingNames.includes(column.name) && !trailingNames.includes(column.name))
  return [...leading, ...middle, ...trailing]
}

async function connect(value = connectionString.value) {
  clearNotices()
  busy.value = true
  message.value = 'Đang kết nối...'
  try {
    const result = await api<{ token: string, tables: TableRef[] }>('/api/connect', {
      method: 'POST',
      body: JSON.stringify({ connectionString: value }),
    })
    if (!result.tables.length) throw new Error('Database không có bảng phù hợp')
    setSessionToken(result.token)
    tables.value = result.tables
    activeConnection.value = value
    saveHistory(value)
    connectionString.value = ''
    await loadTable(result.tables[0].schema, result.tables[0].table)
  } catch (value) {
    error.value = errorText(value)
  } finally {
    busy.value = false
    message.value = ''
  }
}

async function disconnect() {
  clearNotices()
  try {
    await api('/api/disconnect', { method: 'POST' })
  } catch (value) {
    error.value = errorText(value)
  }
  setSessionToken('')
  activeConnection.value = ''
  tables.value = []
  current.value = null
  columns.value = []
  selected.value = []
  stopSortable()
}

async function loadTable(schema: string, table: string) {
  clearNotices()
  busy.value = true
  message.value = `Đang đọc ${schema}.${table}...`
  try {
    const metadata = await api<TableMetadata>(`/api/table?schema=${encodeURIComponent(schema)}&table=${encodeURIComponent(table)}`)
    current.value = metadata
    originalOrder.value = metadata.columns.map((column) => column.name)
    columns.value = suggestedColumns(metadata.columns)
    selected.value = []
    await nextTick()
    startSortable()
  } catch (value) {
    error.value = errorText(value)
  } finally {
    busy.value = false
    message.value = ''
  }
}

function toggleSelection(name: string) {
  selected.value = selected.value.includes(name) ? selected.value.filter((item) => item !== name) : [...selected.value, name]
}

async function beginRename(column: ColumnMetadata) {
  editing.value = column.name
  editName.value = column.name
  await nextTick()
  const input = document.querySelector<HTMLInputElement>('[data-rename-input]')
  input?.focus()
  input?.select()
}

async function finishRename(column: ColumnMetadata, save: boolean) {
  if (editing.value !== column.name) return
  editing.value = null
  const newName = editName.value.trim()
  if (!save || newName === column.name || !current.value) return
  clearNotices()
  busy.value = true
  const pendingOrder = currentOrder.value.map((name) => name === column.name ? newName : name)
  try {
    const metadata = await api<TableMetadata>('/api/rename-column', {
      method: 'POST',
      body: JSON.stringify({
        schema: current.value.schema,
        table: current.value.table,
        oldName: column.name,
        newName,
      }),
    })
    current.value = metadata
    originalOrder.value = metadata.columns.map((candidate) => candidate.name)
    selected.value = selected.value.map((name) => name === column.name ? newName : name)
    const byName = keyBy(metadata.columns, 'name')
    columns.value = pendingOrder.map((name) => byName[name]).filter(Boolean)
    success.value = `Đã đổi tên ${column.name} thành ${newName}.`
  } catch (value) {
    error.value = errorText(value)
  } finally {
    busy.value = false
  }
}

async function preview() {
  if (!current.value) return
  clearNotices()
  busy.value = true
  try {
    plan.value = await api<Plan>('/api/plan', {
      method: 'POST',
      body: JSON.stringify({
        schema: current.value.schema,
        table: current.value.table,
        order: currentOrder.value,
      }),
    })
  } catch (value) {
    error.value = errorText(value)
  } finally {
    busy.value = false
  }
}

async function apply() {
  if (!current.value || !canApply.value) return
  clearNotices()
  busy.value = true
  message.value = 'Đang khóa bảng, copy dữ liệu và đổi bảng...'
  try {
    const metadata = await api<TableMetadata>('/api/apply', {
      method: 'POST',
      body: JSON.stringify({
        schema: current.value.schema,
        table: current.value.table,
        order: currentOrder.value,
      }),
    })
    current.value = metadata
    originalOrder.value = metadata.columns.map((column) => column.name)
    columns.value = metadata.columns
    selected.value = []
    success.value = `Đã reorder ${metadata.schema}.${metadata.table}.`
  } catch (value) {
    error.value = `Đã rollback: ${errorText(value)}`
  } finally {
    busy.value = false
    message.value = ''
  }
}
</script>

<template>
  <div class="min-h-screen bg-background">
    <header class="flex h-13 items-center justify-between border-b bg-card px-4">
      <div class="flex items-center gap-2">
        <Database class="size-4 text-primary" />
        <strong class="text-sm">PG Reorder</strong>
      </div>
      <div class="flex items-center gap-2 text-xs text-muted-foreground">
        <span>{{ connected ? 'Đã kết nối' : 'Chưa kết nối' }}</span>
        <Button v-if="connected" variant="ghost" size="icon" title="Ngắt kết nối" @click="disconnect">
          <LogOut class="size-4" />
        </Button>
      </div>
    </header>

    <main v-if="!connected" class="mx-auto flex min-h-[calc(100vh-3.25rem)] max-w-3xl items-center px-6 py-10">
      <section class="w-full border bg-card p-6 shadow-sm">
        <div class="mb-5">
          <h1 class="text-lg font-semibold">Kết nối PostgreSQL</h1>
          <p class="mt-1 text-sm text-muted-foreground">Connection string được lưu trong localStorage của trình duyệt này.</p>
        </div>

        <div v-if="history.length" class="mb-5 space-y-2">
          <div class="flex items-center justify-between">
            <label class="text-xs font-medium uppercase text-muted-foreground">Kết nối gần đây</label>
            <Button variant="ghost" size="sm" @click="history = []">
              <Trash2 class="size-3.5" />
              Xóa
            </Button>
          </div>
          <div class="grid gap-2 sm:grid-cols-2">
            <button v-for="item in history" :key="item.value" class="truncate rounded-md border bg-background px-3 py-2 text-left text-sm hover:bg-accent" :title="item.label" :disabled="busy" @click="connect(item.value)">
              {{ item.label }}
            </button>
          </div>
        </div>

        <form class="flex gap-2" @submit.prevent="connect()">
          <div class="relative min-w-0 flex-1">
            <Input v-model="connectionString" :type="showConnection ? 'text' : 'password'" autocomplete="off" placeholder="postgresql://user:password@localhost:5432/database" class="pr-10 font-mono text-xs" />
            <button type="button" class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-muted-foreground" :title="showConnection ? 'Ẩn connection string' : 'Hiện connection string'" @click="showConnection = !showConnection">
              <EyeOff v-if="showConnection" class="size-4" />
              <Eye v-else class="size-4" />
            </button>
          </div>
          <Button type="submit" :disabled="busy || !connectionString.trim()">Kết nối</Button>
        </form>
        <Alert v-if="error" variant="destructive" class="mt-4">{{ error }}</Alert>
        <p v-if="message" class="mt-3 text-sm text-muted-foreground">{{ message }}</p>
      </section>
    </main>

    <main v-else class="grid h-[calc(100vh-3.25rem)] grid-cols-[240px_minmax(0,1fr)]">
      <aside class="flex min-h-0 flex-col border-r bg-card">
        <div class="border-b p-3">
          <div class="relative">
            <Search class="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
            <Input v-model="tableSearch" type="search" placeholder="Tìm schema hoặc table" class="pl-8" />
          </div>
        </div>
        <nav class="min-h-0 flex-1 overflow-auto p-2">
          <section v-for="(items, schema) in groupedTables" :key="schema" class="mb-3">
            <div class="px-2 py-1 text-[11px] font-semibold uppercase text-muted-foreground">{{ schema }}</div>
            <button
              v-for="item in items"
              :key="`${item.schema}.${item.table}`"
              :title="`${item.schema}.${item.table}`"
              :disabled="busy"
              :class="['block w-full truncate rounded-md px-2 py-1.5 text-left text-sm disabled:pointer-events-none disabled:opacity-50', current?.schema === item.schema && current?.table === item.table ? 'bg-accent font-medium text-accent-foreground' : 'hover:bg-muted']"
              @click="loadTable(item.schema, item.table)"
            >
              {{ item.table }}
            </button>
          </section>
        </nav>
      </aside>

      <section class="min-w-0 overflow-auto p-4 lg:p-5">
        <div v-if="!current" class="mx-auto max-w-6xl">
          <Alert v-if="error" variant="destructive">{{ error }}</Alert>
          <p v-else-if="message" class="text-sm text-muted-foreground">{{ message }}</p>
        </div>
        <div v-else class="mx-auto max-w-6xl">
          <div class="mb-4 flex items-center justify-between gap-4">
            <div class="min-w-0">
              <p class="truncate text-xs text-muted-foreground">{{ current.schema }}</p>
              <h1 class="truncate text-xl font-semibold">{{ current.table }}</h1>
            </div>
            <div class="flex shrink-0 gap-2">
              <Button variant="outline" :disabled="!canApply" @click="preview">
                <Eye class="size-4" />
                SQL
              </Button>
              <Button variant="destructive" :disabled="!canApply" @click="apply">Apply reorder</Button>
            </div>
          </div>

          <div class="mb-4 grid grid-cols-3 border lg:grid-cols-6">
            <div v-for="([label, value], index) in metrics" :key="label" :class="['min-w-0 px-3 py-2', index > 0 && 'border-l', index > 2 && 'border-t lg:border-t-0']">
              <div class="text-[10px] font-semibold uppercase text-muted-foreground">{{ label }}</div>
              <strong class="mt-0.5 block truncate text-sm">{{ value }}</strong>
            </div>
          </div>

          <Alert variant="warning" class="mb-3 flex items-start gap-2">
            <AlertTriangle class="mt-0.5 size-4 shrink-0" />
            <span>Apply giữ ACCESS EXCLUSIVE lock và copy toàn bộ dữ liệu. Bảng không khả dụng trong lúc chạy.</span>
          </Alert>
          <Alert v-if="current.preflightWarnings.length" variant="warning" class="mb-3">
            <strong class="mb-1 block">Preflight warning</strong>
            <div v-for="warning in current.preflightWarnings" :key="warning">{{ warning }}</div>
          </Alert>
          <Alert v-if="current.blockers.length" variant="destructive" class="mb-3">
            <strong class="mb-1 block">Không thể apply</strong>
            <div v-for="blocker in current.blockers" :key="blocker">{{ blocker }}</div>
          </Alert>
          <Alert v-if="success" variant="success" class="mb-3 flex items-center gap-2"><Check class="size-4" />{{ success }}</Alert>
          <Alert v-if="error" variant="destructive" class="mb-3">{{ error }}</Alert>
          <p v-if="message" class="mb-3 text-sm text-muted-foreground">{{ message }}</p>

          <div class="border bg-card">
            <div class="flex h-11 items-center justify-between border-b px-3">
              <h2 class="text-sm font-semibold">Thứ tự cột</h2>
              <Badge v-if="selected.length" variant="secondary">{{ selected.length }} selected</Badge>
            </div>
            <ol ref="columnsElement" class="divide-y">
              <li
                v-for="column in columns"
                :key="columnKey(column)"
                :class="['grid min-h-12 grid-cols-[32px_minmax(120px,1fr)_minmax(180px,auto)] items-center gap-2 px-3 text-sm transition-colors', selected.includes(column.name) && 'bg-accent']"
                :data-id="columnKey(column)"
                @click="editing !== column.name && toggleSelection(column.name)"
              >
                <button class="drag-handle flex size-8 cursor-grab items-center justify-center text-muted-foreground active:cursor-grabbing" title="Kéo để đổi vị trí" @click.stop>
                  <ChevronsUpDown class="size-4" />
                </button>
                <Input
                  v-if="editing === column.name"
                  v-model="editName"
                  data-rename-input
                  class="h-8 max-w-md"
                  @click.stop
                  @dblclick.stop
                  @blur="finishRename(column, true)"
                  @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
                  @keydown.esc.prevent="finishRename(column, false)"
                />
                <strong v-else class="min-w-0 truncate" title="Double-click để đổi tên" @dblclick.stop="beginRename(column)">{{ column.name }}</strong>
                <div class="truncate text-right font-mono text-xs text-muted-foreground">
                  {{ column.type }}
                  <span v-if="column.not_null"> · NOT NULL</span>
                  <span v-if="column.identity"> · IDENTITY</span>
                  <span v-if="column.default_expr"> · DEFAULT</span>
                </div>
              </li>
            </ol>
          </div>

          <div v-if="plan" class="mt-4 border bg-card">
            <div class="border-b px-3 py-2 text-sm font-semibold">SQL preview</div>
            <pre class="max-h-96 overflow-auto p-3 text-xs leading-5"><code>{{ plan.sql }}</code></pre>
          </div>
        </div>
      </section>
    </main>
  </div>
</template>
