import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'

/**
 * Identity of the console bundle itself. The backend reports its own build over
 * `/api/v1/version`, so the About page can show both and flag a mismatch — the
 * release binary embeds `web/dist` at compile time, and a stale pair is
 * otherwise invisible from inside the running app.
 */
function webBuild() {
  const { version } = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string }
  const git = (args: string[]) => {
    try {
      return execFileSync('git', args, { encoding: 'utf8' }).trim()
    } catch {
      return ''
    }
  }
  // Container builds have no `.git`, so the image build injects the revision.
  const injected = (process.env.PANGOLIN_WEB_COMMIT ?? '').trim()
  const revision = injected || git(['rev-parse', '--short=12', 'HEAD']) || 'unknown'
  const dirty = !injected && git(['status', '--porcelain', '--untracked-files=no']) !== ''
  return {
    version,
    commit: dirty ? `${revision}-dirty` : revision,
    builtAt: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
  }
}

/**
 * Record the console's revision inside the artifact. `web/dist` is gitignored, so
 * the working-tree revision says nothing about the embedded bytes; the Rust build
 * reads this file and reports it next to its own revision.
 */
function recordWebBuild(): Plugin {
  return {
    name: 'pangolin-record-build',
    apply: 'build',
    writeBundle(options) {
      const directory = options.dir ?? 'dist'
      writeFileSync(join(directory, 'build.json'), `${JSON.stringify(webBuild(), null, 2)}\n`)
    },
  }
}

export default defineConfig({
  plugins: [react(), recordWebBuild()],
  define: {
    __WEB_BUILD__: JSON.stringify(webBuild()),
  },
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/metrics': 'http://127.0.0.1:8080',
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
})
