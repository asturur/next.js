import { evaluated, type WalkResult } from './state'

const marker = '13-1'
evaluated.push(marker)

const loaders = [() => import('./n14_0'), () => import('./n14_1')]

export async function walk(branch: number): Promise<WalkResult> {
  const child = await loaders[branch]()
  const result = await child.walk(branch)
  return { ...result, path: [marker, ...result.path] }
}
