import { expect, it } from 'vitest'
import { nextModelImportSelection } from './modelImportSelection'

it('rejects an entire group that would exceed the batch limit', () => {
  const selected = Array.from({ length: 87 }, (_, index) => `first-${index}`)
  expect(nextModelImportSelection(selected, Array.from({ length: 14 }, (_, index) => `second-${index}`), true, 100)).toBeNull()
  expect(nextModelImportSelection(selected, ['first-0', 'last'], true, 100)).toHaveLength(88)
  expect(nextModelImportSelection(selected, ['first-0'], false, 100)).toHaveLength(86)
})
