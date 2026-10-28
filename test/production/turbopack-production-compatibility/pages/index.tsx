import { useState } from 'react'
import data from '../data.json'

function createWorker({ classWorkerURL }: { classWorkerURL?: string } = {}) {
  return classWorkerURL
    ? new Worker(new URL(classWorkerURL, import.meta.url), { type: 'module' })
    : new Worker(new URL('../default-worker.ts', import.meta.url), {
        type: 'module',
      })
}

export default function Page() {
  const [result, setResult] = useState('idle')
  function run(dynamic: boolean) {
    const url = dynamic
      ? URL.createObjectURL(
          new Blob(['postMessage("dynamic-worker")'], {
            type: 'text/javascript',
          })
        )
      : undefined
    const worker = createWorker({ classWorkerURL: url })
    worker.onmessage = (event) => {
      setResult(event.data)
      worker.terminate()
      if (url) URL.revokeObjectURL(url)
    }
  }
  return (
    <main>
      <p id="json">{data.value}</p>
      <button id="dynamic" onClick={() => run(true)}>
        Dynamic worker
      </button>
      <button id="static" onClick={() => run(false)}>
        Static worker
      </button>
      <output>{result}</output>
    </main>
  )
}
