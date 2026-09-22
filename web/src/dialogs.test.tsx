import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import i18n from './i18n'
import { Modal } from './components'

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) return sources(path)
    if (!/\.tsx$/.test(entry) || /\.test\.tsx$/.test(entry)) return []
    return [path]
  })
}

describe('dialogs', () => {
  it('names its close button', async () => {
    await i18n.changeLanguage('en')
    render(
      <Modal open onOpenChange={() => {}} title="Add channel">
        <p>body</p>
      </Modal>,
    )
    expect(screen.getByRole('button', { name: 'Close' })).toBeDefined()
  })

  it('leaves no dialog close button unnamed', () => {
    // axe reports an unnamed close button as a critical violation, and Mantine
    // only names it when the prop is passed. Our own wrapper passes it, so the
    // guard is over the places that reach for Mantine's Modal directly.
    const offenders = sources(join(process.cwd(), 'src')).flatMap((path) => {
      // An arrow function inside a prop contains a `>`, which ends a naive
      // `<Modal[^>]*>` match before it reaches `closeButtonProps`; the guard
      // reported a correctly named dialog as an offender. Arrows cannot affect
      // whether the prop is present, so they are neutralised before matching.
      const source = readFileSync(path, 'utf8').replace(/=>/g, '=')
      const aliases = [...source.matchAll(/import\s*\{([^}]*)\}\s*from\s*'@mantine\/core'/g)]
        .flatMap((match) => match[1].split(',').map((part) => part.trim()))
        .filter((part) => /^Modal(\s+as\s+\w+)?$/.test(part))
        .map((part) => part.split(/\s+as\s+/)[1] ?? 'Modal')
      return aliases.flatMap((alias) =>
        [...source.matchAll(new RegExp(`<${alias}\\b[^>]*>`, 'g'))]
          .filter((match) => !match[0].includes('closeButtonProps'))
          .map((match) => `${path}: ${match[0].slice(0, 80)}`),
      )
    })
    expect(offenders).toEqual([])
  })
})
