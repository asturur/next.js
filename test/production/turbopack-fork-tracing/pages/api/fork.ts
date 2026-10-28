import { fork } from 'node:child_process'
import path from 'node:path'
import type { NextApiRequest, NextApiResponse } from 'next'

export default function handler(_req: NextApiRequest, res: NextApiResponse) {
  const child = fork(path.join(process.cwd(), 'fork-worker.cjs'))
  child.once('message', (message) => {
    res.status(200).json(message)
    child.kill()
  })
  child.once('exit', (code) => {
    if (!res.writableEnded)
      res.status(500).json({ error: `Child exited: ${code}` })
  })
  child.once('error', (error) => {
    res.status(500).json({ error: error.message })
  })
}
