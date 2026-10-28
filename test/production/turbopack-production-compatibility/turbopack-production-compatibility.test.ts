import fs from 'node:fs/promises'
import path from 'node:path'
import { nextTestSetup } from 'e2e-utils'
import { retry } from 'next-test-utils'

describe('turbopack-production-compatibility', () => {
  const { next } = nextTestSetup({
    files: __dirname,
    nextConfig: { experimental: { turbopackFileSystemCacheForBuild: true } },
    env: {
      TURBO_ENGINE_IGNORE_DIRTY: '1',
      TURBO_ENGINE_SNAPSHOT_MIN_ACTIVE_TIME_MILLIS: '0',
    },
  })

  it('imports JSON with a leading UTF-8 BOM', async () => {
    const $ = await next.render$('/')
    expect($('#json').text()).toBe('bom-json')
  })

  it('preserves runtime worker URLs and bundles the static fallback', async () => {
    const errors: string[] = []
    const browser = await next.browser('/', {
      beforePageLoad(page) {
        page.on('pageerror', (error) => errors.push(error.message))
      },
    })
    for (const kind of ['dynamic', 'static']) {
      await browser.elementByCss(`#${kind}`).click()
      await retry(async () => {
        expect(await browser.elementByCss('output').text()).toBe(
          `${kind}-worker`
        )
      })
    }
    expect(errors).toEqual([])
  })

  it('resolves Node source paths at runtime, including shorthand properties', async () => {
    const response = await next.fetch('/api/paths')
    expect(response.status).toBe(200)
    const dirname = path.join(next.testDir, 'pages/api')
    const filename = path.join(dirname, 'paths.ts')
    expect(await response.json()).toEqual({
      dirname,
      filename,
      shorthand: { __dirname: dirname, __filename: filename },
      isVirtual: false,
    })
  })

  it('traces a fork target expressed as a filesystem path', async () => {
    const response = await next.fetch('/api/fork')
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({ result: 'fork-worker' })
    const manifest = JSON.parse(
      await next.readFile('.next/server/pages/api/fork.js.nft.json')
    )
    expect(manifest.files).toEqual(
      expect.arrayContaining([
        expect.stringMatching(/\/fork-worker\.ts$/),
        expect.stringMatching(/\/fork-dependency\.ts$/),
      ])
    )
  })

  it('updates transitive fork traces across cached production builds', async () => {
    await next.stop()
    const cache = await fs.readdir(
      path.join(next.testDir, '.next/cache/turbopack'),
      { recursive: true, withFileTypes: true }
    )
    expect(cache.some((entry) => entry.isFile())).toBe(true)
    const original = await next.readFile('fork-dependency.ts')
    await next.patchFile('fork-added.ts', "exports.suffix = '-changed'\n")
    await next.patchFile(
      'fork-dependency.ts',
      original + "\nexports.childResult += require('./fork-added.ts').suffix\n"
    )
    expect((await next.build()).exitCode).toBe(0)
    let manifest = JSON.parse(
      await next.readFile('.next/server/pages/api/fork.js.nft.json')
    )
    expect(manifest.files).toEqual(
      expect.arrayContaining([expect.stringMatching(/\/fork-added\.ts$/)])
    )
    await next.start()
    expect(await (await next.fetch('/api/fork')).json()).toEqual({
      result: 'fork-worker-changed',
    })
    await next.stop()
    await next.patchFile('fork-dependency.ts', original)
    await next.deleteFile('fork-added.ts')
    expect((await next.build()).exitCode).toBe(0)
    manifest = JSON.parse(
      await next.readFile('.next/server/pages/api/fork.js.nft.json')
    )
    expect(manifest.files).not.toEqual(
      expect.arrayContaining([expect.stringMatching(/\/fork-added\.ts$/)])
    )
  })

  it('still rejects a missing statically bundled worker', async () => {
    await next.stop()
    const page = await next.readFile('pages/index.tsx')
    await next.patchFile(
      'pages/index.tsx',
      page.replace('../default-worker.ts', '../missing-worker.ts')
    )
    const { exitCode, cliOutput } = await next.build()
    expect(exitCode).not.toBe(0)
    expect(cliOutput).toContain('missing-worker.ts')
  })
})
