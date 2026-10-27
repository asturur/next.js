import { nextTestSetup } from 'e2e-utils'
import { retry } from 'next-test-utils'

describe('overlapping-async-chunks', () => {
  const { next } = nextTestSetup({
    files: __dirname,
  })

  // Each of the two modules at a layer imports both modules at the next layer.
  // The leaves import layer 1 again, closing an asynchronous cycle.
  // There are only 36 modules, but 2^18 import histories. Chunk planning must not
  // create a different loader for each history that has identical availability.
  it.each(['/', '/other'])(
    'preserves lazy evaluation, shared state and CSS through %s',
    async (pathname) => {
      const errors: string[] = []
      const browser = await next.browser(pathname, {
        beforePageLoad(page) {
          page.on('pageerror', (error) => errors.push(error.message))
          page.on('response', (response) => {
            if (response.status() >= 400) {
              errors.push(`${response.status()} ${response.url()}`)
            }
          })
        },
      })

      expect(await browser.elementByCss('#evaluated').text()).toBe('0')
      for (const [branch, count] of [
        [0, 18],
        [1, 36],
        [0, 36],
      ]) {
        await browser.elementByCss(`#load-${branch}`).click()
        await retry(async () => {
          expect(await browser.elementByCss('#path').text()).toBe(
            Array.from({ length: 18 }, (_, level) => `${level}-${branch}`).join(
              ' / '
            )
          )
          expect(await browser.elementByCss('#evaluated').text()).toBe(
            `${count}`
          )
          expect(
            await browser.eval(
              () => getComputedStyle(document.querySelector('#path')!).color
            )
          ).toBe('rgb(12, 34, 56)')
        })
      }
      expect(errors).toEqual([])
    }
  )
})
