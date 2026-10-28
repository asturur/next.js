import { evaluated, type WalkResult } from './state'
import styles from './leaf.module.css'

const marker = '17-0'
evaluated.push(marker)

const loaders = [() => import('./n1_0'), () => import('./n1_1')]

export async function walk(branch: number): Promise<WalkResult> {
  // Closing the cycle must reuse the module evaluated earlier in this walk.
  await loaders[branch]()
  return { path: [marker], className: styles.leaf }
}
