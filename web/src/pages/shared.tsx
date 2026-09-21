import { ActionIcon, Alert, Button, Collapse, Group, Modal, NumberInput, Pagination, Paper, PasswordInput, Select, Stack, Switch, Table, TableScrollContainer, Text, Textarea, TextInput, Title, Tooltip } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createColumnHelper, flexRender, getCoreRowModel, getFilteredRowModel, useReactTable } from '@tanstack/react-table'
import { AlertTriangle, ChevronRight, Copy, Inbox, Plus, Search, Trash2 } from 'lucide-react'
import { useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { EmptyState, EnabledPill, SkeletonRows } from '../components'
import i18n from '../i18n'
import { projectOperationPath, useProject } from '../project'

export function PageHeader({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return <Group component="header" justify="space-between" align="flex-start" mb="lg" className="page-header"><Stack gap={0}><Title order={1}>{title}</Title>{description && <Text size="sm" c="dimmed">{description}</Text>}</Stack>{action}</Group>
}

export function QueryError({ retry }: { retry: () => void }) {
  const { t } = useTranslation()
  return <Alert variant="light" color="red" radius="lg" title={t('networkError')} icon={<AlertTriangle />} className="query-error"><Group justify="space-between" align="center" wrap="nowrap"><Text size="sm">{t('retryHint')}</Text><Button variant="outline" color="red" size="compact-sm" onClick={retry}>{t('retry')}</Button></Group></Alert>
}

export const formatDate = (value: unknown) => typeof value === 'number' && value > 0
  ? new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value * 1000))
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
  kind?: 'text' | 'number' | 'secret' | 'textarea' | 'json' | 'checkbox' | 'select'
  required?: boolean
  omitWhenBlank?: boolean
  hint?: string
  options?: Array<{ value: string; label: string }>
  defaultValue?: unknown
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
  rowActions?: (row: Document) => ReactNode
  normalize?: (values: Record<string, unknown>, editing: Document | null) => Record<string, unknown>
}

const allows = (rule: boolean | ((row: Document) => boolean) | undefined, row: Document) => typeof rule === 'function' ? rule(row) : rule !== false
const forbids = (rule: boolean | ((row: Document) => boolean) | undefined, row: Document) => typeof rule === 'function' ? rule(row) : rule === true

function readForm(form: HTMLFormElement, fields: FormField[]) {
  const data = new FormData(form)
  const output: Record<string, unknown> = {}
  for (const field of fields) {
    if (field.kind === 'checkbox') { output[field.key] = data.get(field.key) === 'on'; continue }
    const raw = String(data.get(field.key) ?? '').trim()
    if (!raw && (field.omitWhenBlank || field.kind === 'secret')) continue
    if (!raw && !field.required) { output[field.key] = null; continue }
    if (field.kind === 'number') output[field.key] = raw ? Number(raw) : null
    else if (field.kind === 'json') output[field.key] = raw ? JSON.parse(raw) : { version: 1 }
    else output[field.key] = raw
  }
  return output
}

export function ResourcePage({ resource, title, description, empty, columns, fields = [], createLabel, immutable, appendOnly, editDisabled = false, endpoint, itemEndpoint, createMethod = 'POST', updateMethod = 'POST', canDelete = true, rowActions, normalize }: ResourcePageProps) {
  const { t } = useTranslation()
  const { project } = useProject()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<Document | null>(null)
  const [filter, setFilter] = useState('')
  const [offset, setOffset] = useState(0)
  const [limit, setLimit] = useState(25)
  const lastTrigger = useRef<HTMLButtonElement | null>(null)
  const path = typeof endpoint === 'function' ? endpoint(project.id) : endpoint || projectOperationPath(project.id, resource)
  const queryPath = `${path}${path.includes('?') ? '&' : '?'}offset=${offset}&limit=${limit}${filter.trim() ? `&q=${encodeURIComponent(filter.trim())}` : ''}`
  const query = useQuery({ queryKey: ['resource', project.id, resource, path, offset, limit, filter], queryFn: () => api<Paged<Document> | Document[]>(queryPath) })
  const allRows = useMemo(() => Array.isArray(query.data) ? query.data.filter((row) => !filter.trim() || JSON.stringify(row).toLowerCase().includes(filter.trim().toLowerCase())) : query.data?.data || [], [filter, query.data])
  const rows = Array.isArray(query.data) ? allRows.slice(offset, offset + limit) : allRows
  const total = Array.isArray(query.data) ? allRows.length : query.data?.total ?? rows.length
  const totalPages = Math.max(1, Math.ceil(total / limit))
  const currentPage = Math.floor(offset / limit) + 1
  const save = useMutation({
    mutationFn: ({ body, current }: { body: Record<string, unknown>; current: Document | null }) => api(current && itemEndpoint ? itemEndpoint(current.id, project.id) : path, { method: current ? updateMethod : createMethod, body: JSON.stringify(body) }),
    onSuccess: () => { toast.success(t('saved')); setOpen(false); setEditing(null); requestAnimationFrame(() => lastTrigger.current?.focus()); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
    onError: (error: Error) => toast.error(error.message),
  })
  const remove = useMutation({
    mutationFn: (id: string) => api(itemEndpoint ? itemEndpoint(id, project.id) : projectOperationPath(project.id, resource, id), { method: 'DELETE' }),
    onSuccess: () => { toast.success(t('deleted')); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
    onError: (error: Error) => toast.error(error.message),
  })
  const columnHelper = createColumnHelper<Document>()
  const tableColumns = useMemo(() => [
    ...columns.map((column) => columnHelper.accessor((row) => row[column.key], { id: column.key, header: column.label, cell: (info) => <span className={column.mono ? 'mono-cell' : ''}>{column.render ? column.render(info.getValue(), info.row.original) : displayValue(info.getValue())}</span> })),
    ...((!immutable && !appendOnly && fields.length) || rowActions ? [columnHelper.display({ id: 'actions', header: () => <span className="sr-only">{t('actions')}</span>, cell: ({ row }) => <Group gap={4} justify="flex-end" wrap="nowrap">{!immutable && !appendOnly && !forbids(editDisabled, row.original) && fields.length > 0 && <Button variant="subtle" size="compact-sm" onClick={(event) => { lastTrigger.current = event.currentTarget as HTMLButtonElement; setEditing(row.original); setOpen(true) }}>{t('edit')}</Button>}<Tooltip label={t('copyId')}><ActionIcon variant="subtle" color="gray" aria-label={`${t('copyId')} ${displayValue(row.original.name || row.original.id)}`} onClick={() => void navigator.clipboard?.writeText(String(row.original.id))}><Copy size={15} /></ActionIcon></Tooltip>{!immutable && !appendOnly && allows(canDelete, row.original) && <Tooltip label={t('delete')}><ActionIcon variant="subtle" color="red" aria-label={`${t('delete')} ${displayValue(row.original.name || row.original.id)}`} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(row.original.id)}><Trash2 size={16} /></ActionIcon></Tooltip>}{rowActions?.(row.original)}</Group> })] : []),
  ], [appendOnly, canDelete, columns, editDisabled, fields.length, immutable, rowActions, t])
  const table = useReactTable({ data: rows, columns: tableColumns, getCoreRowModel: getCoreRowModel(), getFilteredRowModel: getFilteredRowModel() })
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    try {
      const values = readForm(event.currentTarget, fields)
      save.mutate({ body: normalize ? normalize(values, editing) : { ...values, ...(editing && !itemEndpoint ? { id: editing.id } : {}) }, current: editing })
    } catch { toast.error(t('invalidJson')) }
  }
  const beginCreate = (trigger: HTMLButtonElement) => { lastTrigger.current = trigger; setEditing(null); setOpen(true) }
  const handleClose = () => { setOpen(false); setEditing(null); requestAnimationFrame(() => lastTrigger.current?.focus()) }
  return <>
    <PageHeader title={title} description={description} action={!immutable && fields.length && (total > 0 || filter) ? <Button leftSection={<Plus size={17} />} onClick={(event) => beginCreate(event.currentTarget)}>{createLabel || t('add')}</Button> : undefined} />
    {(total > 0 || filter) && <TextInput leftSection={<Search size={17} />} placeholder={t('search')} value={filter} onChange={(event) => { setFilter(event.target.value); setOffset(0) }} aria-label={t('search')} mb="md" className="search-control" />}
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : rows.length === 0 ? <EmptyState icon={<Inbox />} title={title} copy={filter ? t('noSearchResults') : empty} action={!filter && !immutable && fields.length ? <Button onClick={(event) => beginCreate(event.currentTarget)}>{createLabel || t('add')}</Button> : undefined} /> : <><TableScrollContainer minWidth={700} className="desktop-resource-table" style={{ maxHeight: 'min(74vh, 900px)' }} role="region" aria-label={title} tabIndex={0}><Table stickyHeader highlightOnHover><Table.Thead>{table.getHeaderGroups().map((group) => <Table.Tr key={group.id}>{group.headers.map((header) => <Table.Th key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</Table.Th>)}</Table.Tr>)}</Table.Thead><Table.Tbody>{table.getRowModel().rows.map((row) => <Table.Tr key={row.id}>{row.getVisibleCells().map((cell) => <Table.Td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</Table.Td>)}</Table.Tr>)}</Table.Tbody></Table></TableScrollContainer><MobileResources rows={rows} columns={columns} rowActions={rowActions} onEdit={!immutable && !appendOnly && fields.length ? (row, trigger) => { lastTrigger.current = trigger; setEditing(row); setOpen(true) } : undefined} editDisabled={editDisabled} onDelete={!immutable && !appendOnly ? (row) => confirm(t('deleteConfirm')) && remove.mutate(row.id) : undefined} canDelete={canDelete} /></>}
    {total > limit && <Group justify="flex-end" gap="sm" mt="md" className="pagination"><Pagination total={totalPages} value={currentPage} onChange={(page) => setOffset((page - 1) * limit)} getControlProps={(control) => {
      if (control === 'previous') return { 'aria-label': t('previous') }
      if (control === 'next') return { 'aria-label': t('next') }
      return {}
    }} /><Text size="sm" c="dimmed">{offset + 1}–{Math.min(offset + limit, total)} / {total}</Text><Select aria-label={t('pageSize')} data={['10', '25', '50']} value={String(limit)} onChange={(value) => { value && setLimit(Number(value)); setOffset(0) }} size="sm" style={{ width: 80 }} /></Group>}
    <Modal opened={open} onClose={handleClose} title={editing ? `${t('edit')} ${title}` : createLabel || `${t('add')} ${title}`} closeButtonProps={{ 'aria-label': t('close') }}>
      <form onSubmit={submit}><Stack gap="md">{fields.map((field) => <ResourceField key={field.key} field={field} value={editing?.[field.sourceKey || field.key] ?? field.defaultValue} />)}<Group justify="flex-end" gap="xs" pt="xs"><Button variant="default" type="button" onClick={handleClose}>{t('cancel')}</Button><Button type="submit" loading={save.isPending}>{t('save')}</Button></Group></Stack></form>
    </Modal>
  </>
}

function MobileResources({ rows, columns, onEdit, onDelete, rowActions, editDisabled, canDelete }: { rows: Document[]; columns: Array<{ key: string; label: string; mono?: boolean; render?: (value: unknown, row: Document) => ReactNode }>; onEdit?: (row: Document, trigger: HTMLButtonElement) => void; onDelete?: (row: Document) => void; rowActions?: (row: Document) => ReactNode; editDisabled?: boolean | ((row: Document) => boolean); canDelete?: boolean | ((row: Document) => boolean) }) {
  const { t } = useTranslation()
  const primary = (row: Document) => String(row.name || row.public_name || row.email || row.pattern || row.suffix || row.id)
  const state = (row: Document) => row.enabled != null ? <EnabledPill enabled={row.enabled} /> : displayValue(row.status)
  const visible = columns.filter((column) => column.key !== 'id').slice(0, 3)
  return <Stack gap="sm" className="mobile-resource-list">{rows.map((row) => <Paper key={row.id} p="md" withBorder><Group justify="space-between" align="flex-start" mb="sm"><Stack gap={4} style={{ minWidth: 0 }}><Text fw={600} style={{ overflowWrap: 'anywhere' }}>{primary(row)}</Text><Button variant="subtle" size="compact-xs" c="dimmed" onClick={() => void navigator.clipboard?.writeText(String(row.id))}>{t('copyId')}</Button></Stack><Text component="div" size="sm">{state(row)}</Text></Group><Stack gap="xs" mb={onEdit || onDelete || rowActions ? 'sm' : undefined} style={{ minWidth: 0 }}>{visible.filter((column) => String(row[column.key] ?? '') !== primary(row)).map((column) => <Group key={column.key} gap="xs" wrap="nowrap" style={{ minWidth: 0 }}><Text size="sm" c="dimmed" style={{ minWidth: '88px', flex: '0 0 auto' }}>{column.label}</Text><Text component="div" size="sm" className={column.mono ? 'mono-cell' : undefined} style={{ flex: 1, minWidth: 0 }}>{column.render ? column.render(row[column.key], row) : displayValue(row[column.key])}</Text></Group>)}</Stack>{(onEdit || onDelete || rowActions) && <Group gap="xs">{onEdit && !forbids(editDisabled, row) && <Button variant="subtle" size="compact-sm" onClick={(event) => onEdit(row, event.currentTarget)}>{t('edit')}</Button>}{onDelete && allows(canDelete, row) && <Button variant="subtle" color="red" size="compact-sm" onClick={() => onDelete(row)}>{t('delete')}</Button>}{rowActions?.(row)}</Group>}</Paper>)}</Stack>
}

function ResourceField({ field, value }: { field: FormField; value: unknown }) {
  const [advanced, setAdvanced] = useState(false)
  if (field.kind === 'select' && field.options) {
    return <Select
      name={field.key}
      label={field.label}
      description={field.hint}
      data={field.options.map((o) => ({ value: o.value, label: o.label }))}
      defaultValue={String(value ?? field.options[0]?.value ?? '')}
      required={field.required}
    />
  }
  if (field.kind === 'checkbox') {
    return <Switch
      name={field.key}
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
    return <PasswordInput
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
            required={field.required}
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
  />
}