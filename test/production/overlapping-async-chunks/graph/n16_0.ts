import { evaluated, type WalkResult } from './state'

const marker = '16-0'
evaluated.push(marker)

const loaders = [() => import('./n17_0'), () => import('./n17_1')]

export async function walk(branch: number): Promise<WalkResult> {
  const child = await loaders[branch]()
  const result = await child.walk(branch)
  return { ...result, path: [marker, ...result.path] }
}
