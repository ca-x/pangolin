import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmHost, confirmAction } from './components'
import i18n from './i18n'

/**
 * The shared confirmation contract. Every destructive control in the console
 * goes through `confirmAction` + `<ConfirmHost />`, so these are the properties
 * the five call sites inherit rather than re-implement: the object is named,
 * the safe choice holds focus, Escape/cancel/outside-click/close all abort
 * without touching the server, and a slow action cannot be fired twice.
 */
const title = 'Delete API key “staging-ci”?'
const body = 'Clients using this key stop authenticating immediately. This cannot be undone.'

/** Request a confirmation the way a click handler does, inside React's act(). */
function ask(overrides: { title?: string; body?: string; confirmLabel?: string; onConfirm?: () => unknown } = {}) {
  act(() => {
    confirmAction({ title, body, onConfirm: () => {}, ...overrides })
  })
}

const dialog = () => screen.getByRole('dialog')
const button = (name: string) => within(dialog()).getByRole('button', { name })

describe('the shared confirm dialog', () => {
  beforeEach(async () => { await i18n.changeLanguage('en') })

  it('names the object it is about to destroy and states the consequence', () => {
    render(<ConfirmHost />)
    ask()
    // Mantine wires the title and body into aria-labelledby/aria-describedby, so
    // the dialog is announced by name and not just by the role.
    expect(screen.getByRole('dialog', { name: title })).toBeInTheDocument()
    expect(within(dialog()).getByText(body)).toBeInTheDocument()
  })

  it('offers a destructive confirm, after a cancel that already holds focus', async () => {
    render(<ConfirmHost />)
    ask()
    const cancel = button('Cancel')
    const confirm = button('Delete')
    // Cancel comes first in reading order, carries the focus trap's marker and
    // owns the focus, so Enter on an untouched dialog aborts.
    expect(cancel.compareDocumentPosition(confirm) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(cancel).toHaveAttribute('data-autofocus')
    await waitFor(() => expect(cancel).toHaveFocus())
    // The destructive action is painted with the danger fill, not the accent.
    expect(confirm.style.getPropertyValue('--button-bg')).toBe('var(--mantine-color-red-filled)')
    expect(confirm).not.toHaveFocus()
  })

  it('runs the action once when confirmed, then closes', async () => {
    const onConfirm = vi.fn(() => Promise.resolve())
    render(<ConfirmHost />)
    ask({ onConfirm })
    await userEvent.click(button('Delete'))
    expect(onConfirm).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('cannot be confirmed twice while the action is in flight', async () => {
    let settle = () => {}
    const onConfirm = vi.fn(() => new Promise<void>((resolve) => { settle = resolve }))
    render(<ConfirmHost />)
    ask({ onConfirm })
    const confirm = button('Delete')
    await userEvent.click(confirm)
    expect(onConfirm).toHaveBeenCalledTimes(1)
    expect(confirm).toBeDisabled()
    expect(confirm).toHaveAttribute('data-loading', 'true')
    // A second activation, even one dispatched straight at the node, must not
    // queue another delete behind the first.
    fireEvent.click(confirm)
    expect(onConfirm).toHaveBeenCalledTimes(1)
    await act(async () => { settle() })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('reports a failure inside the dialog and stays open for another attempt', async () => {
    const onConfirm = vi.fn().mockRejectedValueOnce(new Error('channel still has active credentials'))
    render(<ConfirmHost />)
    ask({ onConfirm })
    await userEvent.click(button('Delete'))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('channel still has active credentials')
    // The dialog is not left in a pending state: the retry is clickable again.
    const confirm = button('Delete')
    expect(confirm).not.toBeDisabled()
    await userEvent.click(confirm)
    expect(onConfirm).toHaveBeenCalledTimes(2)
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('aborts on cancel and hands focus back to the trigger', async () => {
    const onConfirm = vi.fn()
    function Trigger() {
      return <button type="button" onClick={() => confirmAction({ title, body, onConfirm })}>Delete staging-ci</button>
    }
    render(<><Trigger /><ConfirmHost /></>)
    const trigger = screen.getByRole('button', { name: 'Delete staging-ci' })
    await userEvent.click(trigger)
    await waitFor(() => expect(button('Cancel')).toHaveFocus())
    await userEvent.click(button('Cancel'))
    expect(onConfirm).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    await waitFor(() => expect(trigger).toHaveFocus())
  })

  it('aborts on Escape without running the action', async () => {
    const onConfirm = vi.fn()
    render(<ConfirmHost />)
    ask({ onConfirm })
    await userEvent.keyboard('{Escape}')
    expect(onConfirm).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('aborts on a click outside the dialog', async () => {
    const onConfirm = vi.fn()
    render(<ConfirmHost />)
    ask({ onConfirm })
    const overlay = document.querySelector('.mantine-Modal-overlay')
    expect(overlay).not.toBeNull()
    await userEvent.click(overlay as HTMLElement)
    expect(onConfirm).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('aborts on the header close button', async () => {
    const onConfirm = vi.fn()
    render(<ConfirmHost />)
    ask({ onConfirm })
    await userEvent.click(button('Close'))
    expect(onConfirm).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('ignores Escape, the close button and outside clicks while the action is in flight', async () => {
    let settle = () => {}
    const onConfirm = vi.fn(() => new Promise<void>((resolve) => { settle = resolve }))
    render(<ConfirmHost />)
    ask({ onConfirm })
    await userEvent.click(button('Delete'))
    // Hiding the dialog mid-flight would report success for work that has not
    // finished, so every dismissal is refused until the action settles.
    await userEvent.keyboard('{Escape}')
    await userEvent.click(button('Close'))
    await userEvent.click(document.querySelector('.mantine-Modal-overlay') as HTMLElement)
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await act(async () => { settle() })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('accepts a per-action confirm label', () => {
    render(<ConfirmHost />)
    ask({ title: 'Roll “acme” back to revision 1?', confirmLabel: 'Rollback' })
    expect(button('Rollback')).toBeInTheDocument()
  })

  it('refuses to run an action when no host is mounted to ask through', () => {
    // Without the host a caller would either delete unconfirmed or crash after
    // the fact; failing here names the missing piece instead.
    expect(() => confirmAction({ title, body, onConfirm: () => {} })).toThrow(/ConfirmHost/)
  })
})

describe('destructive call sites', () => {
  it('no longer ask through the browser confirm', () => {
    // `window.confirm` cannot name the object, show a pending state or report a
    // failure, and it is synchronous, so it blocks the whole console.
    const offenders = sources(join(process.cwd(), 'src')).flatMap((path) => {
      const source = readFileSync(path, 'utf8')
      return [...source.matchAll(/(?<![A-Za-z])confirm\(/g)].map((match) => `${path}: ${source.slice(Math.max(0, match.index - 40), match.index + 20).replace(/\s+/g, ' ')}`)
    })
    expect(offenders).toEqual([])
  })
})

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) return sources(path)
    if (!/\.tsx?$/.test(entry) || /\.test\.tsx?$/.test(entry)) return []
    return [path]
  })
}
