import { nextTestSetup } from 'e2e-utils'

describe('turbopack-json-bom', () => {
  const { next } = nextTestSetup({ files: __dirname })

  it('imports JSON with a leading UTF-8 BOM', async () => {
    const $ = await next.render$('/')
    expect($('#json').text()).toBe('bom-json')
  })
})
