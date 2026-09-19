import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { EmptyState } from './components'

describe('shared UI', () => {
  it('renders an explicit empty state', () => {
    render(<EmptyState icon={<span />} title="No requests" copy="Send one request to begin." />)
    expect(screen.getByRole('heading', { name: 'No requests' })).toBeInTheDocument()
    expect(screen.getByText('Send one request to begin.')).toBeInTheDocument()
  })
})
