import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createColumnHelper, flexRender, getCoreRowModel, getFilteredRowModel, useReactTable } from '@tanstack/react-table'
import { AlertTriangle, Inbox, Plus, Search, Trash2 } from 'lucide-react'
import { useMemo, useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { EmptyState, Field, Modal, SelectField, SkeletonRows } from '../components'
import { projectOperationPath, useProject } from '../project'

export function PageHeader({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return <header className="page-header"><div><h1>{title}</h1>{description && <p>{description}</p>}</div>{action}</header>
}

export function QueryError({ retry }: { retry: () => void }) {
  const { t } = useTranslation()
  return <div className="query-error" role="alert"><AlertTriangle aria-hidden="true" /><div><strong>{t('networkError')}</strong><p>{t('retryHint')}</p></div><button className="button" onClick={retry}>{t('retry')}</button></div>
}

export const formatDate = (value: unknown) => typeof value === 'number' && value > 0
  ? new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value * 1000))
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
  editDisabled?: boolean
  endpoint?: string | ((projectId: string) => string)
  itemEndpoint?: (id: string, projectId: string) => string
  createMethod?: 'POST' | 'PUT'
  updateMethod?: 'POST' | 'PUT' | 'PATCH'
  canDelete?: boolean
  rowActions?: (row: Document) => ReactNode
  normalize?: (values: Record<string, unknown>, editing: Document | null) => Record<string, unknown>
}

function readForm(form: HTMLFormElement, fields: FormField[]) {
  const data = new FormData(form)
  const output: Record<string, unknown> = {}
  for (const field of fields) {
    if (field.kind === 'checkbox') { output[field.key] = data.get(field.key) === 'on'; continue }
    const raw = String(data.get(field.key) ?? '').trim()
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
  const path = typeof endpoint === 'function' ? endpoint(project.id) : endpoint || projectOperationPath(project.id, resource)
  const query = useQuery({ queryKey: ['resource', project.id, resource, path], queryFn: () => api<Paged<Document> | Document[]>(`${path}${path.includes('?') ? '&' : '?'}limit=200`) })
  const rows = useMemo(() => Array.isArray(query.data) ? query.data : query.data?.data || [], [query.data])
  const save = useMutation({
    mutationFn: ({ body, current }: { body: Record<string, unknown>; current: Document | null }) => api(current && itemEndpoint ? itemEndpoint(current.id, project.id) : path, { method: current ? updateMethod : createMethod, body: JSON.stringify(body) }),
    onSuccess: () => { toast.success(t('saved')); setOpen(false); setEditing(null); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
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
    ...((!immutable && !appendOnly && fields.length) || rowActions ? [columnHelper.display({ id: 'actions', header: () => <span className="sr-only">{t('actions')}</span>, cell: ({ row }) => <div className="row-actions">{!immutable && !appendOnly && !editDisabled && fields.length > 0 && <button className="button button-quiet" onClick={() => { setEditing(row.original); setOpen(true) }}>{t('edit')}</button>}{!immutable && !appendOnly && canDelete && <button className="icon-button danger" aria-label={`${t('delete')} ${displayValue(row.original.name || row.original.id)}`} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(row.original.id)}><Trash2 size={16}/></button>}{rowActions?.(row.original)}</div> })] : []),
  ], [appendOnly, canDelete, columns, editDisabled, fields.length, immutable, rowActions, t])
  const table = useReactTable({ data: rows, columns: tableColumns, state: { globalFilter: filter }, onGlobalFilterChange: setFilter, getCoreRowModel: getCoreRowModel(), getFilteredRowModel: getFilteredRowModel() })
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    try {
      const values = readForm(event.currentTarget, fields)
      save.mutate({ body: normalize ? normalize(values, editing) : { ...values, ...(editing && !itemEndpoint ? { id: editing.id } : {}) }, current: editing })
    } catch { toast.error(t('invalidJson')) }
  }
  const beginCreate = () => { setEditing(null); setOpen(true) }
  return <>
    <PageHeader title={title} description={description} action={!immutable && fields.length ? <button className="button button-primary" onClick={beginCreate}><Plus size={17}/>{createLabel || t('add')}</button> : undefined} />
    {rows.length > 0 && <label className="search-control"><Search size={17} aria-hidden="true"/><span className="sr-only">{t('search')}</span><input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder={t('search')} /></label>}
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : rows.length === 0 ? <EmptyState icon={<Inbox />} title={title} copy={empty} action={!immutable && fields.length ? <button className="button" onClick={beginCreate}>{createLabel || t('add')}</button> : undefined} /> : <div className="table-wrap" role="region" aria-label={title} tabIndex={0}><table><thead>{table.getHeaderGroups().map((group) => <tr key={group.id}>{group.headers.map((header) => <th key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</th>)}</tr>)}</thead><tbody>{table.getRowModel().rows.map((row) => <tr key={row.id}>{row.getVisibleCells().map((cell) => <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>)}</tr>)}</tbody></table></div>}
    <Modal open={open} onOpenChange={(value) => { setOpen(value); if (!value) setEditing(null) }} title={editing ? `${t('edit')} ${title}` : createLabel || `${t('add')} ${title}`}>
      <form className="form-stack" onSubmit={submit}>{fields.map((field) => <ResourceField key={field.key} field={field} value={editing?.[field.sourceKey || field.key] ?? field.defaultValue}/>) }<div className="form-actions"><button type="button" className="button" onClick={() => setOpen(false)}>{t('cancel')}</button><button className="button button-primary" disabled={save.isPending}>{save.isPending ? t('loading') : t('save')}</button></div></form>
    </Modal>
  </>
}

function ResourceField({ field, value }: { field: FormField; value: unknown }) {
  const [selected, setSelected] = useState(String(value ?? field.options?.[0]?.value ?? ''))
  if (field.kind === 'select' && field.options) return <SelectField name={field.key} label={field.label} value={selected} onValueChange={setSelected} options={field.options}/>
  if (field.kind === 'checkbox') return <label className="check-field"><input name={field.key} type="checkbox" defaultChecked={value == null ? true : Boolean(value)}/><span>{field.label}</span></label>
  const initial = field.kind === 'json' && typeof value === 'object' ? JSON.stringify(value, null, 2) : String(value ?? '')
  return <Field label={field.label} hint={field.hint}>{field.kind === 'textarea' || field.kind === 'json' ? <textarea name={field.key} required={field.required} defaultValue={initial} rows={field.kind === 'json' ? 7 : 4}/> : <input name={field.key} type={field.kind === 'secret' ? 'password' : field.kind === 'number' ? 'number' : 'text'} required={field.required} defaultValue={initial}/>}</Field>
}
