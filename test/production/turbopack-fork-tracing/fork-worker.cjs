const { childResult } = require('./fork-dependency.cjs')

process.send?.({ result: childResult })
