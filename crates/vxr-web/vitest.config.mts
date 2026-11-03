// The Worker's tests (worker/test/), in workerd through the Cloudflare pool, against local D1
// and R2 only: `npm test` in crates/vxr-web. Each test file starts from the migrations.
import path from 'node:path';
import { cloudflareTest, readD1Migrations } from '@cloudflare/vitest-plugin';
import { defineConfig } from 'vitest/config';

export default defineConfig(async () => {
  const migrations = await readD1Migrations(path.join(import.meta.dirname, 'migrations'));
  return {
    plugins: [
      cloudflareTest({
        wrangler: { configPath: './wrangler.jsonc' },
        miniflare: {
          bindings: {
            TEST_MIGRATIONS: migrations,
            BETTER_AUTH_SECRET: 'test-secret-test-secret-test-secret-0123',
            // Cloudflare's documented Turnstile test secret that always passes.
            TURNSTILE_SECRET_KEY: '1x0000000000000000000000000000000AA',
          },
        },
      }),
    ],
    test: { include: ['worker/test/*.test.ts'], setupFiles: ['worker/test/setup.ts'] },
  };
});
