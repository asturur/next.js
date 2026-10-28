import path from 'node:path'
import { nextTestSetup } from 'e2e-utils'

describe('turbopack-node-source-paths', () => {
  const { next } = nextTestSetup({ files: __dirname })

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
})
