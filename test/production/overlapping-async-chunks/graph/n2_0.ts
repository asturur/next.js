import { evaluated, type WalkResult } from './state'

const marker = '2-0'
evaluated.push(marker)

const loaders = [() => import('./n3_0'), () => import('./n3_1')]

export async function walk(branch: number): Promise<WalkResult> {
  const child = await loaders[branch]()
  const result = await child.walk(branch)
  return { ...result, path: [marker, ...result.path] }
}
