import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { CapabilityTags } from '../components'
import i18n from '../i18n'

let capabilityStyles: HTMLStyleElement

beforeEach(() => {
  capabilityStyles = document.createElement('style')
  const styles = readFileSync(join(process.cwd(), 'src/styles.css'), 'utf8')
  capabilityStyles.textContent = styles.match(/\.pm-capabilities\s*\{[^}]*\}/)?.[0] || ''
  document.head.append(capabilityStyles)
})

afterEach(() => { capabilityStyles.remove() })

describe('operational capability text', () => {
  it.each(['en', 'zh-CN'])('renders capability values and the remaining count at 14px in %s', async (language) => {
    await i18n.changeLanguage(language)
    render(<table><tbody><tr><td><CapabilityTags value={['chat', 'responses', 'embeddings', 'images']} /></td></tr></tbody></table>)
    const label = screen.getByText(i18n.t('capabilityChat'))
    const capabilities = label.closest('.pm-capabilities')
    expect(capabilityStyles.textContent).toMatch(/font-size:\s*14px/)
    expect(capabilities).not.toBeNull()
    expect(getComputedStyle(capabilities!).fontSize).toBe('14px')
    expect(screen.getByText('+1')).toBeInTheDocument()
    for (const value of capabilities!.querySelectorAll('.pm-capability')) {
      expect(getComputedStyle(value).fontSize || getComputedStyle(capabilities!).fontSize).toBe('14px')
    }
  })
})
