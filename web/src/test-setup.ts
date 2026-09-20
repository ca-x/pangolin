import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach, vi } from 'vitest'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', { value: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value), removeItem: (key: string) => storage.delete(key), clear: () => storage.clear() }, configurable: true })
Object.defineProperty(window, 'matchMedia', { value: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) })
Object.defineProperties(Element.prototype, { hasPointerCapture: { value: () => false }, setPointerCapture: { value: () => {} }, releasePointerCapture: { value: () => {} }, scrollIntoView: { value: () => {} } })
afterEach(() => { cleanup(); vi.unstubAllGlobals() })
