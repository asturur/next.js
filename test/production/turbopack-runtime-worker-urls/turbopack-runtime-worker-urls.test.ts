import { nextTestSetup } from 'e2e-utils'
import { retry } from 'next-test-utils'

describe('turbopack-runtime-worker-urls', () => {
  const { next } = nextTestSetup({ files: __dirname })

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
