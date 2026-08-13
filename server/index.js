import crypto from 'node:crypto'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import express from 'express'
import { applyPlan, buildPlan, connect, inspectTable, listTables, renameColumn } from './database.js'

const app = express()
const sessions = new Map()
const dist = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../dist')

app.use(express.json({ limit: '100kb' }))

const route = (handler) => async (request, response) => {
  try {
    await handler(request, response)
  } catch (error) {
    response.status(400).json({ error: error instanceof Error ? error.message : String(error) })
  }
}

function session(request) {
  const token = request.headers.authorization?.replace(/^Bearer /, '')
  const client = token ? sessions.get(token) : null
  if (!client) throw new Error('Not connected')
  return client
}

app.get('/api/health', (_request, response) => response.json({ ok: true }))

app.post('/api/connect', route(async (request, response) => {
  const connectionString = request.body.connectionString
  if (typeof connectionString !== 'string' || !connectionString.trim()) throw new Error('Connection string is required')
  const client = await connect(connectionString.trim())
  try {
    const tables = await listTables(client)
    const token = crypto.randomUUID()
    sessions.set(token, client)
    response.json({ token, tables })
  } catch (error) {
    await client.end()
    throw error
  }
}))

app.post('/api/disconnect', route(async (request, response) => {
  const token = request.headers.authorization?.replace(/^Bearer /, '')
  const client = token ? sessions.get(token) : null
  if (client) await client.end()
  if (token) sessions.delete(token)
  response.json({ ok: true })
}))

app.get('/api/table', route(async (request, response) => {
  response.json(await inspectTable(session(request), String(request.query.schema), String(request.query.table)))
}))

app.post('/api/plan', route(async (request, response) => {
  const metadata = await inspectTable(session(request), request.body.schema, request.body.table)
  response.json(buildPlan(metadata, request.body.order))
}))

app.post('/api/apply', route(async (request, response) => {
  const client = session(request)
  const metadata = await inspectTable(client, request.body.schema, request.body.table)
  await applyPlan(client, metadata, request.body.order)
  response.json(await inspectTable(client, request.body.schema, request.body.table))
}))

app.post('/api/rename-column', route(async (request, response) => {
  response.json(await renameColumn(session(request), request.body.schema, request.body.table, request.body.oldName, request.body.newName))
}))

app.use(express.static(dist))
app.get('*path', (_request, response) => response.sendFile(path.join(dist, 'index.html')))

const port = Number(process.env.PORT || 3210)
const host = process.env.HOST || '127.0.0.1'
const server = app.listen(port, host, () => console.log(`http://${host}:${port}`))

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, async () => {
    server.close()
    await Promise.allSettled([...sessions.values()].map((client) => client.end()))
    process.exit(0)
  })
}
