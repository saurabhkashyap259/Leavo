// Create the dedicated E2E database (leavo_e2e) if missing, so resetting it never touches
// the `leavo` dev database. Runs before the E2E server starts (playwright.config.ts).
import pg from 'pg'

const url = new URL(process.env.E2E_DATABASE_URL)
const name = url.pathname.slice(1)
const admin = new URL(url)
admin.pathname = '/postgres'
const client = new pg.Client({ connectionString: admin.toString() })
await client.connect()
try {
  const { rowCount } = await client.query('SELECT 1 FROM pg_database WHERE datname = $1', [name])
  if (!rowCount) {
    await client.query(`CREATE DATABASE "${name.replace(/"/g, '""')}"`)
    console.log(`created database ${name}`)
  }
} finally {
  await client.end()
}
