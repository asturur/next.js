import { useState } from 'react'
import { evaluated, type WalkResult } from '../graph/state'

const loaders = [() => import('../graph/n0_0'), () => import('../graph/n0_1')]

export default function Page() {
  const [result, setResult] = useState<WalkResult>({ path: [], className: '' })
  const [count, setCount] = useState(0)

  async function load(branch: number) {
    const graph = await loaders[branch]()
    setResult(await graph.walk(branch))
    setCount(evaluated.length)
  }

  return (
    <main>
      <button id="load-0" onClick={() => load(0)}>
        Load left
      </button>
      <button id="load-1" onClick={() => load(1)}>
        Load right
      </button>
      <output id="path" className={result.className}>
        {result.path.join(' / ')}
      </output>
      <output id="evaluated">{count}</output>
    </main>
  )
}
