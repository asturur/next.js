import type { NextApiRequest, NextApiResponse } from 'next'

export default function handler(_req: NextApiRequest, res: NextApiResponse) {
  const shorthand = { __dirname, __filename }
  res.json({
    dirname: __dirname,
    filename: __filename,
    shorthand,
    isVirtual: __dirname === '/ROOT/pages/api',
  })
}
