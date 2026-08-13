# pg-column-reorder

Web UI to visually reorder PostgreSQL table columns via drag-and-drop. Generates and executes a transactional migration that recreates the table with the desired column order.

## How it works

1. Connect to any PostgreSQL database using a connection string.
2. Browse schemas/tables in the sidebar.
3. Drag columns to reorder, double-click to rename.
4. Preview the generated SQL, then apply.

The migration runs inside a single transaction: creates a temp table with the new column order, copies all data, swaps names, rebuilds constraints/indexes, and drops the old table. If anything fails, the transaction is rolled back.

## Preflight checks

Before allowing a reorder, the tool inspects the table for unsupported features and blocks the operation if any are found:

- Partitioned or inherited tables
- Row-level security policies
- Custom replica identity, tablespace, or storage options
- Non-heap access methods
- Publications, extended statistics, user triggers, dependent views
- Generated columns, custom collation, column privileges
- Tables larger than 5 GB (warning at 1 GB)

## Stack

- **Frontend:** Vue 3, Tailwind CSS 4, Vite
- **Backend:** Express 5, node-postgres
- **Desktop:** Tauri 2 (optional)

## Development

```bash
npm install
npm run dev        # Vite dev server (frontend)
npm start          # Express server (port 3210)
```

## Docker

```bash
docker compose up -d              # production (behind Traefik)
docker compose --profile test up  # includes a test Postgres instance
```

The test Postgres instance is available at `localhost:55432` with credentials `reorder:reorder`.

Connection string for testing:

```
postgresql://reorder:reorder@localhost:55432/reorder
```

## Build

```bash
npm run build      # Vue + TypeScript build
npm run typecheck   # Type check only
npm run lint        # ESLint
```
