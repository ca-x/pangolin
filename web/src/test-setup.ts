import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach, vi } from 'vitest'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', { value: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value), removeItem: (key: string) => storage.delete(key), clear: () => storage.clear() }, configurable: true })
Object.defineProperty(window, 'matchMedia', { value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) })
// Mantine's ScrollArea, Select and Combobox observe their own size; jsdom has neither.
class ObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return []
  }
}
Object.defineProperty(globalThis, 'ResizeObserver', { value: ObserverStub, configurable: true })
Object.defineProperty(globalThis, 'IntersectionObserver', { value: ObserverStub, configurable: true })
Object.defineProperties(Element.prototype, { hasPointerCapture: { value: () => false }, setPointerCapture: { value: () => {} }, releasePointerCapture: { value: () => {} }, scrollIntoView: { value: () => {} } })
afterEach(() => { cleanup(); vi.unstubAllGlobals() })

// Auto-wrap all test renders with MantineProvider so migrated Mantine components render outside the real shell.
// `env="test"` disables Mantine's transitions and renders its portals inline, which is what lets the
// synchronous role/label queries in these tests see modal and combobox content right after a click.
vi.mock('@testing-library/react', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@testing-library/react')>()
  const React = await import('react')
  const { MantineProvider } = await import('@mantine/core')
  const composed = (outer: React.ComponentType<any> | undefined, inner: React.ComponentType<any>): React.ComponentType<any> =>
    outer ? (({ children }: any) => React.createElement(outer, null, React.createElement(inner, { env: 'test' }, children))) : (({ children }: any) => React.createElement(inner, { env: 'test' }, children))
  return {
    ...actual,
    render: (ui: React.ReactElement, options?: any) =>
      actual.render(ui, { ...options, wrapper: composed(options?.wrapper, MantineProvider as React.ComponentType<any>) }),
  }
})