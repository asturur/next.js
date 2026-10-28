import fs from 'node:fs/promises'
import path from 'node:path'
import { nextTestSetup } from 'e2e-utils'

describe('turbopack-fork-tracing', () => {
  const { next } = nextTestSetup({
    files: __dirname,
    nextConfig: { experimental: { turbopackFileSystemCacheForBuild: true } },
    env: {
      TURBO_ENGINE_IGNORE_DIRTY: '1',
      TURBO_ENGINE_SNAPSHOT_MIN_ACTIVE_TIME_MILLIS: '0',
    },
  })

  async function tracedFiles() {
    const manifest = JSON.parse(
      await next.readFile('.next/server/pages/api/fork.js.nft.json')
    )
    return manifest.files.map((file: string) => file.replace(/\\/g, '/'))
  }

  it('runs the original fork target and traces its transitive dependencies', async () => {
    const response = await next.fetch('/api/fork')
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({ result: 'fork-worker' })
    expect(await tracedFiles()).toEqual(
      expect.arrayContaining([
        expect.stringMatching(/\/fork-worker\.cjs$/),
        expect.stringMatching(/\/fork-dependency\.cjs$/),
      ])
    )
  })

  it('updates transitive fork traces across cached production builds', async () => {
    await next.stop()
    const cache = await fs.readdir(
      path.join(next.testDir, '.next/cache/turbopack'),
      {
        recursive: true,
        withFileTypes: true,
      }
    )
    expect(cache.some((entry) => entry.isFile())).toBe(true)
    const original = await next.readFile('fork-dependency.cjs')
    await next.patchFile('fork-added.cjs', "exports.suffix = '-changed'\n")
    await next.patchFile(
      'fork-dependency.cjs',
      original + "\nexports.childResult += require('./fork-added.cjs').suffix\n"
    )
    expect((await next.build()).exitCode).toBe(0)
    expect(await tracedFiles()).toEqual(
      expect.arrayContaining([expect.stringMatching(/\/fork-added\.cjs$/)])
    )
    await next.start()
    expect(await (await next.fetch('/api/fork')).json()).toEqual({
      result: 'fork-worker-changed',
    })
    await next.stop()
    await next.patchFile('fork-dependency.cjs', original)
    await next.deleteFile('fork-added.cjs')
    expect((await next.build()).exitCode).toBe(0)
    expect(await tracedFiles()).not.toEqual(
      expect.arrayContaining([expect.stringMatching(/\/fork-added\.cjs$/)])
    )
  })
})
