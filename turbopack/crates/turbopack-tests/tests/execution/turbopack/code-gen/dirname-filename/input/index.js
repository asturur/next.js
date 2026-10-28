const { existsSync } = require('node:fs')
const { dirname } = require('node:path')
const { fileURLToPath } = require('node:url')

it('__dirname and __filename should resolve to real source paths', () => {
  expect(__filename).toEqual(fileURLToPath(import.meta.url))
  expect(__dirname).toEqual(dirname(__filename))
  expect(existsSync(__filename)).toBe(true)
  expect({ __dirname, __filename }).toEqual({
    __dirname: dirname(fileURLToPath(import.meta.url)),
    __filename: fileURLToPath(import.meta.url),
  })
})
