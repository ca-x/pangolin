import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

const styles = readFileSync(join(process.cwd(), 'src/styles.css'), 'utf8')
const containerRule = styles.match(/\.mantine-TableScrollContainer-scrollContainer\s*\{[^}]*\}/)?.[0] || ''
const focusRule = styles.match(/\.mantine-TableScrollContainer-scrollContainer:has\(>\s*\.mantine-ScrollArea-viewport:focus-visible\)::after\s*\{[^}]*\}/)?.[0] || ''

describe('operational table keyboard focus', () => {
  it('anchors the focus indicator to the visible scroll container', () => {
    expect(containerRule).toMatch(/position:\s*relative/)
  })

  it('paints an inset noninteractive frame above sticky headers without motion', () => {
    expect(focusRule).toMatch(/content:\s*['"]{2}/)
    expect(focusRule).toMatch(/position:\s*absolute/)
    expect(focusRule).toMatch(/inset:\s*0\s*;/)
    expect(focusRule).toMatch(/border:\s*2px solid var\(--focus\)/)
    expect(focusRule).toMatch(/border-radius:\s*inherit/)
    expect(focusRule).toMatch(/z-index:\s*4\s*;/)
    expect(focusRule).toMatch(/pointer-events:\s*none/)
    expect(focusRule).not.toMatch(/transition|animation/)
  })

  it.each(['Model catalog', '模型目录'])('frames only the focused viewport for %s', (label) => {
    expect(focusRule).not.toBe('')
    const selector = focusRule.split('{')[0].replace('::after', '').trim()
    render(<div className="mantine-TableScrollContainer-scrollContainer" data-testid="container"><div className="mantine-ScrollArea-viewport" role="region" aria-label={label} tabIndex={0}><table><tbody><tr><td><button type="button">Action</button></td></tr></tbody></table></div></div>)
    const container = screen.getByTestId('container')
    const viewport = screen.getByRole('region', { name: label })
    expect(container.matches(selector)).toBe(false)
    viewport.focus()
    expect(viewport).toHaveFocus()
    expect(container.matches(selector)).toBe(true)
    screen.getByRole('button', { name: 'Action' }).focus()
    expect(container.matches(selector)).toBe(false)
  })
})
