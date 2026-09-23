import { ActionIcon, Alert, Button, Checkbox, Collapse, Group, Loader, Modal, NumberInput, Pagination, Paper, Radio, Select, SimpleGrid, Stack, Switch, Table, TableScrollContainer, Text, Textarea, TextInput, Title, Tooltip } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createColumnHelper, flexRender, getCoreRowModel, getFilteredRowModel, useReactTable } from '@tanstack/react-table'
import { AlertTriangle, ChevronRight, Copy, Inbox, Plus, Search, Trash2 } from 'lucide-react'
import { useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, displayTimezone, type Document, type Paged } from '../api'
import { confirmAction, EmptyState, EnabledPill, InlineQueryError, SecretInput, SkeletonRows } from '../components'
import i18n from '../i18n'
import { projectOperationPath, useProject } from '../project'
import { ProviderIcon } from '../ProviderIcon'

export function PageHeader({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return <Group component="header" justify="space-between" align="flex-start" mb="lg" className="page-header"><Stack gap={0}><Title order={1}>{title}</Title>{description && <Text size="sm" c="dimmed">{description}</Text>}</Stack>{action}</Group>
}

export function QueryError({ retry }: { retry: () => void }) {
  const { t } = useTranslation()
  return <Alert variant="light" color="red" radius="lg" title={t('networkError')} icon={<AlertTriangle />} className="query-error"><Group justify="space-between" align="center" wrap="nowrap"><Text size="sm">{t('retryHint')}</Text><Button variant="outline" color="red" size="compact-sm" onClick={retry}>{t('retry')}</Button></Group></Alert>
}

export const formatDate = (value: unknown) => typeof value === 'number' && value > 0
  ? new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short', timeZone: displayTimezone }).format(new Date(value * 1000))
  : '—'

export const displayValue = (value: unknown) => {
  if (value == null || value === '') return '—'
  if (typeof value === 'boolean') return value ? '✓' : '—'
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

export type FormField = {
  key: string
  sourceKey?: string
  label: string
  kind?: 'text' | 'number' | 'secret' | 'textarea' | 'json' | 'checkbox' | 'select' | 'provider' | 'permission-list' | 'multi-checkbox'
  required?: boolean
  omitWhenBlank?: boolean
  hint?: string
  options?: Array<{ value: string; label: string; logoKey?: string; defaultBaseUrl?: string | null }>
  permissions?: Array<{ slug: string; level: string; description: string }>
  /**
   * `kind: 'select'`: the option list could not be read. The field renders the
   * failure and its retry instead of an empty picker that looks like a real
   * choice, and the form refuses to submit a value it never offered.
   */
  error?: string
  onRetry?: () => void
  /** Keep create-time provenance choices out of edit payloads. */
  createOnly?: boolean
  /** Rendered only while editing an existing row — the create form omits it. */
  editOnly?: boolean
  /** Auxiliary option lists retain a manual choice while these states are shown. */
  loading?: boolean
  emptyMessage?: string
  allowManualOnError?: boolean
  /** `kind: 'provider'`: form field that receives the picked provider's default base URL on create. */
  baseUrlKey?: string
  defaultValue?: unknown
  /** Client-side validation shown next to the input and mirrored into native form validity. */
  validate?: (value: string) => string | undefined
  /**
   * The field's current value when it is not a column of the row it edits — a
   * field of a JSON payload, say. Without it an edit would start from the
   * field's default and silently overwrite the stored value.
   */
  fromRow?: (row: Document) => unknown
  /** Resource-specific structured editor that still submits through this field's normal FormData contract. */
  render?: (props: { name: string; label: string; value: unknown; setValid: (valid: boolean) => void }) => ReactNode
}

type ResourcePageProps = {
  resource: string
  title: string
  description: string
  empty: string
  columns: Array<{ key: string; label: string; mono?: boolean; render?: (value: unknown, row: Document) => ReactNode }>
  fields?: FormField[]
  createLabel?: string
  immutable?: boolean
  appendOnly?: boolean
  editDisabled?: boolean | ((row: Document) => boolean)
  endpoint?: string | ((projectId: string) => string)
  itemEndpoint?: (id: string, projectId: string) => string
  createMethod?: 'POST' | 'PUT'
  updateMethod?: 'POST' | 'PUT' | 'PATCH'
  canDelete?: boolean | ((row: Document) => boolean)
  /** Offers row selection and a selection bar, driven through `bulk-toggle`. */
  selectable?: BulkResource
  /** Channels can preview dependencies and then request an atomic project-scoped bulk delete. */
  bulkDelete?: boolean
  rowActions?: (row: Document) => ReactNode
  /**
   * The card list's own state line for a resource whose state is not `enabled`.
   * The table renders its columns, but the card shows one state word above them,
   * and a resource whose state is an enum needs the same localized word there.
   */
  mobileStatus?: (row: Document) => ReactNode
  /**
   * A filter bar for a resource whose list has durable state of its own. It is
   * rendered even when the list is empty, because a filter that disappears with
   * the rows is exactly what makes a filtered-out row unreachable.
   */
  filters?: ReactNode
  normalize?: (values: Record<string, unknown>, editing: Document | null) => Record<string, unknown>
  /** A slot for an auxiliary query's own banner (health, a lookup). The element carries its own spacing, so an absent banner leaves no gap. */
  notice?: ReactNode
  /** Resource-specific controls can use the shared list shell without adopting its generic editor. */
  headerAction?: ReactNode
  /** Adds a resource-specific action beside the shared create action, including in the empty state. */
  secondaryAction?: ReactNode
  /** Replaces enable/disable for a selected resource whose lifecycle has its own contract. */
  bulkActions?: (selected: string[], clear: () => void) => ReactNode
  emptyAction?: ReactNode
  /** Cards may lead with a richer identity than the resource id. */
  mobilePrimary?: (row: Document) => ReactNode
  mobilePrimaryKey?: string
  mobileHiddenKeys?: string[]
  /** A wide operational table stays inside its own horizontal scroll region. */
  tableMinWidth?: number
  /** Dense resources can keep more facts in their narrow-screen cards. */
  mobileColumnLimit?: number
}

const allows = (rule: boolean | ((row: Document) => boolean) | undefined, row: Document) => typeof rule === 'function' ? rule(row) : rule !== false
const forbids = (rule: boolean | ((row: Document) => boolean) | undefined, row: Document) => typeof rule === 'function' ? rule(row) : rule === true

/** A field's starting value: the row's own column, or what the field derives from the row. */
export const fieldValue = (field: FormField, row: Document | null) => row
  ? (field.fromRow ? field.fromRow(row) : row[field.sourceKey || field.key]) ?? field.defaultValue
  : field.defaultValue

function readForm(form: HTMLFormElement, fields: FormField[]) {
  const data = new FormData(form)
  const output: Record<string, unknown> = {}
  for (const field of fields) {
    if (field.kind === 'checkbox') { output[field.key] = data.get(field.key) === 'on'; continue }
    if (field.kind === 'permission-list' || field.kind === 'multi-checkbox') { output[field.key] = data.getAll(field.key).map(String); continue }
    const raw = String(data.get(field.key) ?? '').trim()
    if (!raw && (field.omitWhenBlank || field.kind === 'secret')) continue
    if (!raw && !field.required) { output[field.key] = null; continue }
    if (field.kind === 'number') output[field.key] = raw ? Number(raw) : null
    else if (field.kind === 'json') output[field.key] = raw ? JSON.parse(raw) : { version: 1 }
    else output[field.key] = raw
  }
  return output
}

export function ResourcePage({ resource, title, description, empty, columns, fields = [], createLabel, immutable, appendOnly, editDisabled = false, endpoint, itemEndpoint, createMethod = 'POST', updateMethod = 'POST', canDelete = true, rowActions, mobileStatus, filters, normalize, notice, selectable, bulkDelete = false, headerAction, secondaryAction, bulkActions, emptyAction, mobilePrimary, mobilePrimaryKey, mobileHiddenKeys, tableMinWidth = 700, mobileColumnLimit = 3 }: ResourcePageProps) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<Document | null>(null)
  const [selected, setSelected] = useState<string[]>([])
  const [filter, setFilter] = useState('')
  const [offset, setOffset] = useState(0)
  const [limit, setLimit] = useState(25)
  const [invalidFields, setInvalidFields] = useState<Set<string>>(() => new Set())
  const lastTrigger = useRef<HTMLButtonElement | null>(null)
  const formRef = useRef<HTMLFormElement | null>(null)
  const path = typeof endpoint === 'function' ? endpoint(project.id) : endpoint || projectOperationPath(project.id, resource)
  const queryPath = `${path}${path.includes('?') ? '&' : '?'}offset=${offset}&limit=${limit}${filter.trim() ? `&q=${encodeURIComponent(filter.trim())}` : ''}`
  const query = useQuery({ queryKey: ['resource', project.id, resource, path, offset, limit, filter], queryFn: () => api<Paged<Document> | Document[]>(queryPath) })
  const allRows = useMemo(() => Array.isArray(query.data) ? query.data.filter((row) => !filter.trim() || JSON.stringify(row).toLowerCase().includes(filter.trim().toLowerCase())) : query.data?.data || [], [filter, query.data])
  const rows = Array.isArray(query.data) ? allRows.slice(offset, offset + limit) : allRows
  const total = Array.isArray(query.data) ? allRows.length : query.data?.total ?? rows.length
  const totalPages = Math.max(1, Math.ceil(total / limit))
  const currentPage = Math.floor(offset / limit) + 1
  const activeFields = fields.filter((field) => (!editing || !field.createOnly) && (editing || !field.editOnly))
  const save = useMutation({
    mutationFn: ({ body, current }: { body: Record<string, unknown>; current: Document | null }) => api(current && itemEndpoint ? itemEndpoint(current.id, project.id) : path, { method: current ? updateMethod : createMethod, body: JSON.stringify(body) }),
    onSuccess: () => { toast.success(t('saved')); setOpen(false); setEditing(null); requestAnimationFrame(() => lastTrigger.current?.focus()); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
    onError: (error: Error) => toast.error(error.message),
  })
  const remove = useMutation({
    mutationFn: (id: string) => api(itemEndpoint ? itemEndpoint(id, project.id) : projectOperationPath(project.id, resource, id), { method: 'DELETE' }),
    onSuccess: () => { toast.success(t('deleted')); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
    // Every delete goes through `confirmRemove`, and that dialog renders the
    // refusal where the decision was made; a toast repeated the same sentence.
  })
  // One generic resource page serves channels, credentials, models, prompts and
  // quotas, so the dialog names the row and the collection it belongs to.
  // The selection bar drives the endpoint the pasted-id panel already used, so
  // there is one bulk path rather than two.
  const bulk = useMutation({
    mutationFn: (enabled: boolean) => api(projectOperationPath(project.id, 'bulk-toggle'), { method: 'POST', body: JSON.stringify({ resource: selectable, ids: selected, enabled }) }),
    onSuccess: () => { toast.success(t('saved')); setSelected([]); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
    onError: (error: Error) => toast.error(error.message),
  })
  const bulkRemove = useMutation({
    mutationFn: (ids: string[]) => api(projectOperationPath(project.id, 'bulk-delete'), { method: 'POST', body: JSON.stringify({ resource: 'channels', ids }) }),
    onSuccess: () => { toast.success(t('deleted')); setSelected([]); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
    onError: (error: Error) => toast.error(error.message),
  })
  const previewBulkRemove = async () => {
    try {
      const preview = await api<{ dependencies: Array<{ models: number; credentials: number }> }>(projectOperationPath(project.id, 'channel-preview'), { method: 'POST', body: JSON.stringify({ action: 'bulk_delete', ids: selected }) })
      const models = preview.dependencies.reduce((sum, row) => sum + row.models, 0)
      const credentials = preview.dependencies.reduce((sum, row) => sum + row.credentials, 0)
      confirmAction({ title: t('bulkDeleteChannelsTitle', { count: selected.length }), body: t('bulkDeleteChannelsBody', { models, credentials }), onConfirm: () => bulkRemove.mutateAsync(selected) })
    } catch (error) { toast.error(error instanceof Error ? error.message : t('networkError')) }
  }
  const confirmRemove = (row: Document) => confirmAction({ title: t('deleteResourceTitle', { name: displayValue(row.name || row.id) }), body: t('deleteResourceBody', { resource: title }), onConfirm: () => remove.mutateAsync(row.id) })
  const columnHelper = createColumnHelper<Document>()
  const tableColumns = useMemo(() => [
    ...(selectable ? [columnHelper.display({
      id: 'select',
      header: () => <Checkbox aria-label={t('selectAll')} checked={rows.length > 0 && rows.every((row) => selected.includes(String(row.id)))} onChange={(event) => setSelected(event.currentTarget.checked ? rows.map((row) => String(row.id)) : [])} />,
      cell: ({ row }) => <Checkbox aria-label={`${t('selectRow')} ${displayValue(row.original.name || row.original.public_name || row.original.id)}`} checked={selected.includes(String(row.original.id))} onChange={(event) => setSelected((current) => event.currentTarget.checked ? [...current, String(row.original.id)] : current.filter((id) => id !== String(row.original.id)))} />,
    })] : []),
    ...columns.map((column) => columnHelper.accessor((row) => row[column.key], { id: column.key, header: column.label, cell: (info) => <span className={column.mono ? 'mono-cell' : ''}>{column.render ? column.render(info.getValue(), info.row.original) : displayValue(info.getValue())}</span> })),
    ...((!immutable && !appendOnly && fields.length) || rowActions ? [columnHelper.display({ id: 'actions', header: () => <span className="sr-only">{t('actions')}</span>, cell: ({ row }) => <Group gap={4} justify="flex-end" wrap="nowrap">{!immutable && !appendOnly && !forbids(editDisabled, row.original) && fields.length > 0 && <Button variant="subtle" size="compact-sm" onClick={(event) => { lastTrigger.current = event.currentTarget as HTMLButtonElement; setInvalidFields(new Set()); setEditing(row.original); setOpen(true) }}>{t('edit')}</Button>}<Tooltip label={t('copyId')}><ActionIcon variant="subtle" color="gray" aria-label={`${t('copyId')} ${displayValue(row.original.name || row.original.id)}`} onClick={() => void navigator.clipboard?.writeText(String(row.original.id))}><Copy size={15} /></ActionIcon></Tooltip>{!immutable && !appendOnly && allows(canDelete, row.original) && <Tooltip label={t('delete')}><ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${displayValue(row.original.name || row.original.id)}`} onClick={() => confirmRemove(row.original)}><Trash2 size={16} /></ActionIcon></Tooltip>}{rowActions?.(row.original)}</Group> })] : []),
  ], [appendOnly, canDelete, columns, editDisabled, fields.length, rowActions, rows, selectable, selected, t])
  const table = useReactTable({ data: rows, columns: tableColumns, getCoreRowModel: getCoreRowModel(), getFilteredRowModel: getFilteredRowModel() })
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    try {
      const values = readForm(event.currentTarget, activeFields)
      save.mutate({ body: normalize ? normalize(values, editing) : { ...values, ...(editing && !itemEndpoint ? { id: editing.id } : {}) }, current: editing })
    } catch (error) {
      // A malformed JSON box keeps the JSON message; a value the form refuses to
      // send reports its own localized reason (a count out of range, a missing
      // target) instead of the generic one.
      toast.error(error instanceof SyntaxError || !(error instanceof Error) ? t('invalidJson') : error.message || t('invalidJson'))
    }
  }
  const beginCreate = (trigger: HTMLButtonElement) => { lastTrigger.current = trigger; setInvalidFields(new Set()); setEditing(null); setOpen(true) }
  // Picking a provider is a create-time convenience: the default endpoint fills
  // the base URL input. An edit keeps whatever the operator saved, so the write
  // is limited to a new row.
  const applyProviderDefault = (field: FormField, provider: string) => {
    if (editing || !field.baseUrlKey) return
    const baseUrl = field.options?.find((option) => option.value === provider)?.defaultBaseUrl
    if (!baseUrl) return
    const input = formRef.current?.elements.namedItem(field.baseUrlKey)
    if (input instanceof HTMLInputElement) input.value = baseUrl
  }
  const handleClose = () => { setOpen(false); setEditing(null); requestAnimationFrame(() => lastTrigger.current?.focus()) }
  const createAction = !immutable && fields.length ? <Button leftSection={<Plus size={17} />} onClick={(event) => beginCreate(event.currentTarget)}>{createLabel || t('add')}</Button> : undefined
  const defaultActions = (secondaryAction || createAction) ? <Group gap="xs" wrap="wrap" justify="flex-end">{secondaryAction}{createAction}</Group> : undefined
  return <>
    <PageHeader title={title} description={description} action={(total > 0 || filter) ? headerAction ?? defaultActions : undefined} />
    {/* A slot for an auxiliary query's own banner (health, a lookup). The element
        carries its own spacing so an empty slot leaves no gap. */}
    {notice}
    {filters && <Group gap="sm" mb="md" className="resource-filters">{filters}</Group>}
    {(total > 0 || filter) && <TextInput leftSection={<Search size={17} />} placeholder={t('search')} value={filter} onChange={(event) => { setFilter(event.target.value); setOffset(0) }} aria-label={t('search')} mb="md" w={{ base: '100%', sm: 360 }} />}
    {selectable && selected.length > 0 && <Group gap="sm" mb="md" role="region" aria-label={t('bulkActions')} className="selection-bar">
      <Text size="sm" fw={540}>{t('selectedCount', { count: selected.length })}</Text>
      {bulkActions ? bulkActions(selected, () => setSelected([])) : <>
        <Button size="compact-sm" variant="default" loading={bulk.isPending} onClick={() => bulk.mutate(true)}>{t('enable')}</Button>
        <Button size="compact-sm" variant="default" loading={bulk.isPending} onClick={() => bulk.mutate(false)}>{t('disable')}</Button>
        {bulkDelete && <Button size="compact-sm" variant="outline" color="red" loading={bulkRemove.isPending} onClick={() => void previewBulkRemove()}>{t('delete')}</Button>}
        <Button size="compact-sm" variant="subtle" onClick={() => setSelected([])}>{t('cancel')}</Button>
      </>}
    </Group>}
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : rows.length === 0 ? <EmptyState icon={<Inbox />} title={title} copy={filter ? t('noSearchResults') : empty} action={!filter ? emptyAction ?? defaultActions : undefined} /> : <><TableScrollContainer minWidth={tableMinWidth} className="desktop-resource-table" style={{ maxHeight: 'min(74vh, 900px)' }} role="region" aria-label={title} tabIndex={0}><Table stickyHeader highlightOnHover><Table.Thead>{table.getHeaderGroups().map((group) => <Table.Tr key={group.id}>{group.headers.map((header) => <Table.Th key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</Table.Th>)}</Table.Tr>)}</Table.Thead><Table.Tbody>{table.getRowModel().rows.map((row) => <Table.Tr key={row.id}>{row.getVisibleCells().map((cell) => <Table.Td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</Table.Td>)}</Table.Tr>)}</Table.Tbody></Table></TableScrollContainer><MobileResources rows={rows} columns={columns} rowActions={rowActions} mobileStatus={mobileStatus} onEdit={!immutable && !appendOnly && fields.length ? (row, trigger) => { lastTrigger.current = trigger; setInvalidFields(new Set()); setEditing(row); setOpen(true) } : undefined} editDisabled={editDisabled} onDelete={!immutable && !appendOnly ? confirmRemove : undefined} canDelete={canDelete} primary={mobilePrimary} primaryKey={mobilePrimaryKey} hiddenKeys={mobileHiddenKeys} columnLimit={mobileColumnLimit} /></>}
    {total > limit && <Group justify="flex-end" gap="sm" mt="md" className="pagination"><Pagination total={totalPages} value={currentPage} onChange={(page) => setOffset((page - 1) * limit)} getControlProps={(control) => {
      if (control === 'previous') return { 'aria-label': t('previous') }
      if (control === 'next') return { 'aria-label': t('next') }
      return {}
    }} /><Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + limit, total)} / {total}</Text><Select aria-label={t('pageSize')} data={['10', '25', '50']} value={String(limit)} onChange={(value) => { value && setLimit(Number(value)); setOffset(0) }} size="sm" style={{ width: 80 }} /></Group>}
    <Modal opened={open} onClose={handleClose} title={editing ? `${t('edit')} ${title}` : createLabel || `${t('add')} ${title}`} closeButtonProps={{ 'aria-label': t('close') }}>
      <form ref={formRef} onSubmit={submit}><Stack gap="md">{activeFields.map((field) => <ResourceField key={`${editing?.id ?? 'create'}:${field.key}`} field={field} value={fieldValue(field, editing)} editing={Boolean(editing)} onProviderChange={applyProviderDefault} onValidityChange={(valid) => setInvalidFields((current) => { const alreadyValid = !current.has(field.key); if (valid === alreadyValid) return current; const next = new Set(current); if (valid) next.delete(field.key); else next.add(field.key); return next })} />)}<Group justify="flex-end" gap="xs" pt="xs"><Button variant="default" type="button" onClick={handleClose}>{t('cancel')}</Button>{/* Required lookups fail closed. Optional provenance pickers retain their explicit manual fallback. */}<Button type="submit" loading={save.isPending} disabled={invalidFields.size > 0 || activeFields.some((field) => field.error && !field.allowManualOnError)}>{t('save')}</Button></Group></Stack></form>
    </Modal>
  </>
}

function MobileResources({ rows, columns, onEdit, onDelete, rowActions, mobileStatus, editDisabled, canDelete, primary: renderPrimary, primaryKey, hiddenKeys = [], columnLimit }: { rows: Document[]; columns: Array<{ key: string; label: string; mono?: boolean; render?: (value: unknown, row: Document) => ReactNode }>; onEdit?: (row: Document, trigger: HTMLButtonElement) => void; onDelete?: (row: Document) => void; rowActions?: (row: Document) => ReactNode; mobileStatus?: (row: Document) => ReactNode; editDisabled?: boolean | ((row: Document) => boolean); canDelete?: boolean | ((row: Document) => boolean); primary?: (row: Document) => ReactNode; primaryKey?: string; hiddenKeys?: string[]; columnLimit: number }) {
  const { t } = useTranslation()
  const primaryText = (row: Document) => String(row.name || row.public_name || row.display_name || row.email || row.pattern || row.suffix || row.id)
  const primary = (row: Document) => renderPrimary ? renderPrimary(row) : primaryText(row)
  // A resource with an `enabled` flag states it with the pill; one whose state is an
  // enum states it with the word its owner localizes, and falls back to the raw
  // value only where no owner asked for one.
  const state = (row: Document) => mobileStatus ? mobileStatus(row) : row.enabled != null ? <EnabledPill enabled={row.enabled} /> : displayValue(row.status)
  const visible = columns.filter((column) => column.key !== 'id' && column.key !== primaryKey && !hiddenKeys.includes(column.key)).slice(0, columnLimit)
  return <Stack gap="sm" className="mobile-resource-list">{rows.map((row) => <Paper key={row.id} p="md" withBorder><Group justify="space-between" align="flex-start" mb="sm"><Stack gap={4} style={{ minWidth: 0 }}><Text component="div" fw={600} style={{ overflowWrap: 'anywhere' }}>{primary(row)}</Text><Button variant="subtle" size="compact-xs" c="dimmed" onClick={() => void navigator.clipboard?.writeText(String(row.id))}>{t('copyId')}</Button></Stack><Text component="div" size="sm">{state(row)}</Text></Group><Stack gap="xs" mb={onEdit || onDelete || rowActions ? 'sm' : undefined} style={{ minWidth: 0 }}>{visible.filter((column) => primaryKey || String(row[column.key] ?? '') !== primaryText(row)).map((column) => <Group key={column.key} gap="xs" wrap="nowrap" style={{ minWidth: 0 }}><Text size="sm" c="dimmed" style={{ minWidth: '88px', flex: '0 0 auto' }}>{column.label}</Text><Text component="div" size="sm" className={column.mono ? 'mono-cell' : undefined} style={{ flex: 1, minWidth: 0 }}>{column.render ? column.render(row[column.key], row) : displayValue(row[column.key])}</Text></Group>)}</Stack>{(onEdit || onDelete || rowActions) && <Group gap="xs">{onEdit && !forbids(editDisabled, row) && <Button variant="subtle" size="compact-sm" onClick={(event) => onEdit(row, event.currentTarget)}>{t('edit')}</Button>}{onDelete && allows(canDelete, row) && <Button variant="subtle" color="red" size="compact-sm" onClick={() => onDelete(row)}>{t('delete')}</Button>}{rowActions?.(row)}</Group>}</Paper>)}</Stack>
}

function permissionValues(value: unknown): string[] {
  if (Array.isArray(value)) return value.filter((permission): permission is string => typeof permission === 'string')
  if (typeof value !== 'string') return []
  try {
    const parsed: unknown = JSON.parse(value)
    return Array.isArray(parsed) ? parsed.filter((permission): permission is string => typeof permission === 'string') : []
  } catch {
    return []
  }
}

function ResourceField({ field, value, editing, onProviderChange, onValidityChange }: { field: FormField; value: unknown; editing: boolean; onProviderChange: (field: FormField, provider: string) => void; onValidityChange: (valid: boolean) => void }) {
  const { t } = useTranslation()
  const [advanced, setAdvanced] = useState(false)
  const [provider, setProvider] = useState(String(value ?? field.options?.[0]?.value ?? ''))
  const [selectedPermissions, setSelectedPermissions] = useState(() => permissionValues(value))
  const [validationError, setValidationError] = useState<string>()
  if (field.render) return field.render({ name: field.key, label: field.label, value, setValid: onValidityChange })
  if (field.kind === 'multi-checkbox') {
    const selected = Array.isArray(value) ? value.map(String) : []
    return <fieldset className="permission-picker">
      <legend>{field.label}</legend>
      {field.hint && <Text size="sm" c="dimmed" mb="xs">{field.hint}</Text>}
      {field.loading && <Group gap="xs" role="status"><Loader size={16} /><Text size="sm" c="dimmed">{t('loading')}</Text></Group>}
      {field.error && <InlineQueryError message={field.error} onRetry={field.onRetry} />}
      {!field.loading && !field.error && (field.options?.length ?? 0) === 0 && <Text size="sm" c="dimmed">{field.emptyMessage}</Text>}
      {!field.error && <Stack gap="xs" className="permission-options">
        {(field.options || []).map((option) => <Checkbox key={option.value} name={field.key} value={option.value} defaultChecked={selected.includes(option.value)} label={option.label} />)}
      </Stack>}
    </fieldset>
  }
  if (field.kind === 'permission-list') {
    const permissions = field.permissions ?? []
    const available = new Set(permissions.map((permission) => permission.slug))
    const unknown = editing ? selectedPermissions.filter((permission) => !available.has(permission)) : []
    const toggle = (slug: string, checked: boolean) => setSelectedPermissions((current) => checked
      ? current.includes(slug) ? current : [...current, slug]
      : current.filter((permission) => permission !== slug))
    return <fieldset className="permission-picker">
      <legend>{field.label}</legend>
      {field.hint && <Text size="sm" c="dimmed" mb="xs">{field.hint}</Text>}
      {field.loading && <Group gap="xs" role="status"><Loader size={16} /><Text size="sm" c="dimmed">{t('loading')}</Text></Group>}
      {field.error && <InlineQueryError message={field.error} onRetry={field.onRetry} />}
      {!field.loading && !field.error && permissions.length === 0 && unknown.length === 0 && <Text size="sm" c="dimmed">{field.emptyMessage}</Text>}
      {(permissions.length > 0 || unknown.length > 0) && <Stack gap="xs" className="permission-options">
        {permissions.map((permission) => <Checkbox
          key={permission.slug}
          className="permission-option"
          name={field.key}
          value={permission.slug}
          checked={selectedPermissions.includes(permission.slug)}
          onChange={(event) => toggle(permission.slug, event.currentTarget.checked)}
          label={<span className="permission-copy"><code>{permission.slug}</code><span>{permission.description}</span></span>}
        />)}
        {unknown.map((slug) => <div key={slug} className="permission-option permission-option-unknown">
          <Checkbox
            checked
            disabled
            label={<span className="permission-copy"><code>{slug}</code><span>{t('permissionUnavailable')}</span></span>}
          />
          <input type="hidden" name={field.key} value={slug} />
        </div>)}
      </Stack>}
    </fieldset>
  }
  if (field.kind === 'provider' && field.options) {
    return <>
      <Radio.Group
        label={field.label}
        description={field.hint}
        required={field.required}
        value={provider}
        onChange={(next) => { setProvider(next); onProviderChange(field, next) }}
      >
        {/* A single-select list of providers, each with its bundled catalog icon.
            Eighteen entries do not fit a dialog unscrolled, and the list has to
            stay usable at 375px, so it is bounded and scrolls in one column. */}
        <div style={{ maxHeight: 248, overflowY: 'auto' }}>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing={6} verticalSpacing={6}>
            {field.options.map((option) => <Radio.Card key={option.value} value={option.value} withBorder radius="md" p="xs" aria-label={option.label}>
              <span className="provider-cell"><ProviderIcon logoKey={option.logoKey} name={option.label} size={20} /><Text size="sm" style={{ minWidth: 0 }}>{option.label}</Text></span>
            </Radio.Card>)}
          </SimpleGrid>
        </div>
      </Radio.Group>
      {/* The radio cards are buttons, so the submitted value travels in a hidden
          field: the payload keeps the shape the API already accepts. */}
      <input type="hidden" name={field.key} value={provider} />
    </>
  }
  if (field.kind === 'select' && field.options) {
    if (field.error && !field.allowManualOnError) return <Stack gap={4}><Text size="sm" fw={500}>{field.label}</Text><InlineQueryError message={field.error} onRetry={field.onRetry} /></Stack>
    return <Stack gap={6}>
      <Select
        name={field.key}
        label={field.label}
        description={field.hint}
        data={field.options.map((o) => ({ value: o.value, label: o.label }))}
        defaultValue={String(value ?? field.options[0]?.value ?? '')}
        required={field.required}
      />
      {field.loading && <Group gap="xs" role="status"><Loader size={14} /><Text size="sm" c="dimmed">{i18n.t('loading')}</Text></Group>}
      {field.error && <InlineQueryError message={field.error} onRetry={field.onRetry} />}
      {!field.loading && !field.error && field.options.length === 1 && field.emptyMessage && <Text size="sm" c="dimmed">{field.emptyMessage}</Text>}
    </Stack>
  }
  if (field.kind === 'checkbox') {
    return <Switch
      name={field.key}
      aria-label={field.label}
      label={field.label}
      description={field.hint}
      defaultChecked={value == null ? true : Boolean(value)}
    />
  }
  if (field.kind === 'number') {
    return <NumberInput
      name={field.key}
      label={field.label}
      description={field.hint}
      defaultValue={value != null && value !== '' ? Number(value) : ''}
      required={field.required}
      thousandSeparator=""
      hideControls
    />
  }
  if (field.kind === 'secret') {
    return <SecretInput
      name={field.key}
      label={field.label}
      description={field.hint}
      defaultValue={String(value ?? '')}
      required={field.required}
    />
  }
  if (field.kind === 'textarea') {
    return <Textarea
      name={field.key}
      label={field.label}
      description={field.hint}
      defaultValue={String(value ?? '')}
      required={field.required}
      rows={4}
    />
  }
  if (field.kind === 'json' && field.required) {
    // Collapsing a required editor would leave the browser blocking the submit
    // with no focusable control to report it, so it stays open.
    const initial = typeof value === 'object' && value !== null ? JSON.stringify(value, null, 2) : String(value ?? '')
    return <Textarea
      name={field.key}
      label={field.label}
      description={field.hint}
      defaultValue={initial}
      required
      rows={7}
    />
  }
  if (field.kind === 'json') {
    const initial = typeof value === 'object' && value !== null ? JSON.stringify(value, null, 2) : String(value ?? '')
    return <Stack gap="xs">
      <Button
        variant="subtle"
        size="compact-sm"
        justify="flex-start"
        leftSection={<ChevronRight size={15} style={{ transform: advanced ? 'rotate(90deg)' : undefined, transition: 'transform 150ms' }} />}
        onClick={() => setAdvanced((open) => !open)}
        aria-expanded={advanced}
      >
        {field.label}
      </Button>
      <Collapse expanded={advanced} keepMounted>
        <div inert={!advanced}>
          <Textarea
            name={field.key}
            aria-label={field.label}
            description={field.hint}
            defaultValue={initial}
            rows={7}
          />
        </div>
      </Collapse>
    </Stack>
  }
  return <TextInput
    name={field.key}
    label={field.label}
    description={field.hint}
    defaultValue={String(value ?? '')}
    required={field.required}
    error={validationError}
    onChange={(event) => {
      const error = field.validate?.(event.currentTarget.value)
      event.currentTarget.setCustomValidity(error || '')
      setValidationError(error)
    }}
  />
}

/** Row count for a resource-backed tab. The key reuses the
 *  `['resource', projectId, resource]` prefix that ResourcePage invalidates
 *  after every create and delete, so the count cannot go stale. */
export function useResourceTotal(resource: BulkResource) {
  const { project } = useProject()
  const query = useQuery({ queryKey: ['resource', project.id, resource], queryFn: () => api<Paged<Document>>(`${projectOperationPath(project.id, resource)}?limit=1`) })
  // `null` means the count is unknown (still loading, or the request failed), and
  // an unknown count must not be treated as empty: that would drop the bulk
  // controls from a populated page whenever this auxiliary request failed.
  // TanStack Query keeps the last `data` through a failed refetch, so a stale
  // zero after an error would read as "no rows" — the error state comes first.
  if (query.isError) return null
  return query.data ? (query.data.total ?? query.data.data?.length ?? 0) : null
}

/** The resources `operations/bulk-toggle` accepts (`src/api/operations_api.rs`). */
export type BulkResource = 'channels' | 'models' | 'credentials' | 'prompts' | 'protection'

/**
 * Bulk enable or disable for a resource-backed tab. It takes ids rather than a
 * row selection, so it is the same panel on every page that offers it: the
 * resource is what changes.
 */
export function BulkToggle({ resource = 'channels' }: { resource?: BulkResource }) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const total = useResourceTotal(resource)
  const mutate = useMutation({ mutationFn: (body: unknown) => api(projectOperationPath(project.id, 'bulk-toggle'), { method: 'POST', body: JSON.stringify(body) }), onSuccess: () => { toast.success(t('saved')); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) }, onError: (error: Error) => toast.error(error.message) })
  const submit = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const data = new FormData(event.currentTarget); mutate.mutate({ resource, ids: String(data.get('ids')).split(',').map((id) => id.trim()).filter(Boolean), enabled: data.get('enabled') === 'true' }) }
  // With no rows to act on the bulk form is dead weight next to the empty
  // state's single call to action, so it only appears once a row exists. An
  // unknown count (`null`) keeps it: the failure belongs to the count query, and
  // silently dropping a working control is worse than showing it unnecessarily.
  if (total === 0) return null
  return (
    <Paper withBorder p="md" mt="md">
      <Title order={3} mb="md">{t('bulkToggleTitle')}</Title>
      <form onSubmit={submit}>
        <Group gap="md" align="end" wrap="wrap">
          <TextInput name="ids" label={t('resourceIds')} placeholder={t('resourceIdsPlaceholder')} required style={{ minWidth: 260, flex: '1 1 240px' }} />
          <Select name="enabled" label={t('action')} defaultValue="true" data={[{ value: 'true', label: t('enable') }, { value: 'false', label: t('disable') }]} />
          {/* The page's primary action is creating a resource, so the bulk
              panel submits through a secondary button. */}
          <Button type="submit" variant="default">{t('apply')}</Button>
        </Group>
      </form>
    </Paper>
  )
}
