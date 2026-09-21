import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { describe, expect, it } from 'vitest'
import { Field, SelectField } from './components'

// The console reads every create/edit form with `new FormData(form)`. The
// components now come from Mantine, so these tests pin the contract that the
// migration had to preserve: a Mantine Select still submits its value, a
// checkbox still submits "on", and a plain input still carries its name.
// Providers come from the shared test setup, which mounts MantineProvider in
// test mode so modal and combobox content renders synchronously.
function Harness({ onSubmit }: { onSubmit: (values: Record<string, unknown>) => void }) {
  const [mode, setMode] = useState('generated')
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault()
        const data = new FormData(event.currentTarget)
        onSubmit({ mode: data.get('mode'), name: data.get('name'), enabled: data.get('enabled') })
      }}
    >
      <Field label="Name">
        <input name="name" defaultValue="Primary" />
      </Field>
      <SelectField name="mode" label="Token mode" value={mode} onValueChange={setMode} options={[{ value: 'generated', label: 'Generated' }, { value: 'import_existing', label: 'Import existing' }]} />
      <label>
        <input name="enabled" type="checkbox" defaultChecked />
        Enabled
      </label>
      <button type="submit">Save</button>
    </form>
  )
}

describe('form contract after the component migration', () => {
  it('submits the select value, the text value and the checkbox flag', async () => {
    const values: Array<Record<string, unknown>> = []
    render(<Harness onSubmit={(submitted) => values.push(submitted)} />)

    await userEvent.click(screen.getByRole('combobox', { name: 'Token mode' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Import existing' }))
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(values).toEqual([{ mode: 'import_existing', name: 'Primary', enabled: 'on' }])
  })

  it('submits the first option when an enum select is left untouched', async () => {
    // The pre-migration selector defaulted to its first option; a select that
    // submits null for a required enum makes the default Save action fail.
    const values: Array<Record<string, unknown>> = []
    render(
      <form
        onSubmit={(event) => {
          event.preventDefault()
          values.push({ kind: new FormData(event.currentTarget).get('kind') })
        }}
      >
        <SelectField name="kind" label="Kind" value="" onValueChange={() => {}} options={[{ value: 'automatic_backup', label: 'Automatic backup' }, { value: 'probe', label: 'Probe' }]} />
        <button type="submit">Save</button>
      </form>,
    )
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(values).toEqual([{ kind: 'automatic_backup' }])
  })

  it('associates the label with the control it wraps', () => {
    render(<Harness onSubmit={() => {}} />)
    expect(screen.getByLabelText('Name')).toHaveValue('Primary')
  })
})
