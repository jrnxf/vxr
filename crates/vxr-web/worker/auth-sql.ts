// Prints the SQL that creates Better Auth's tables for authOptions, for a new file in migrations/
// when Better Auth or its plugins change:
//
//   npx esbuild worker/auth-sql.ts --bundle --platform=node --format=esm --packages=external --outfile=.wrangler/auth-sql.mjs && node .wrangler/auth-sql.mjs
//
// It runs the migration planner against an empty in-memory SQLite (D1 is SQLite), so it prints the
// whole schema; diff it against the tables the existing migrations make.
import { DatabaseSync } from 'node:sqlite';
import { getMigrations } from 'better-auth/db/migration';
import { authOptions } from './auth';

const { compileMigrations } = await getMigrations(authOptions('http://localhost', 'x'.repeat(32), new DatabaseSync(':memory:'), null));
console.log(await compileMigrations());
