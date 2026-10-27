const { childResult } = require('./fork-dependency.ts')

process.send?.({ result: childResult })
