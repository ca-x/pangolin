import * as Tooltip from '@radix-ui/react-tooltip'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createColumnHelper, flexRender, getCoreRowModel, getFilteredRowModel, useReactTable } from '@tanstack/react-table'
import { AlertTriangle, Copy, Inbox, Plus, Search, Trash2 } from 'lucide-react'
import { useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { EmptyState, EnabledPill, Field, Modal, SelectField, SkeletonRows, Tip } from '../components'
import i18n from '../i18n'
import { projectOperationPath, useProject } from '../project'

export function PageHeader({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return <header className="page-header"><div><h1>{title}</h1>{description && <p>{description}</p>}</div>{action}</header>
}

export function QueryError({ retry }: { retry: () => void }) {
  const { t } = useTranslation()
  return <div className="query-error" role="alert"><AlertTriangle aria-hidden="true" /><div><strong>{t('networkError')}</strong><p>{t('retryHint')}</p></div><button className="button" onClick={retry}>{t('retry')}</button></div>
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
  const save = useMutation({
    mutationFn: ({ body, current }: { body: Record<string, unknown>; current: Document | null }) => api(current && itemEndpoint ? itemEndpoint(current.id, project.id) : path, { method: current ? updateMethod : createMethod, body: JSON.stringify(body) }),
    onSuccess: () => { toast.success(t('saved')); setOpen(false); setEditing(null); requestAnimationFrame(()=>lastTrigger.current?.focus()); void client.invalidateQueries({ queryKey: ['resource', project.id, resource] }) },
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
    ...((!immutable && !appendOnly && fields.length) || rowActions ? [columnHelper.display({ id: 'actions', header: () => <span className="sr-only">{t('actions')}</span>, cell: ({ row }) => <div className="row-actions">{!immutable && !appendOnly && !editDisabled && fields.length > 0 && <button className="button button-quiet" onClick={(event) => { lastTrigger.current=event.currentTarget;setEditing(row.original);setOpen(true) }}>{t('edit')}</button>}<Tip label={t('copyId')}><button className="icon-button" aria-label={`${t('copyId')} ${displayValue(row.original.name || row.original.id)}`} onClick={()=>void navigator.clipboard?.writeText(String(row.original.id))}><Copy size={15}/></button></Tip>{!immutable && !appendOnly && canDelete && <Tip label={t('delete')}><button className="icon-button danger" aria-label={`${t('delete')} ${displayValue(row.original.name || row.original.id)}`} onClick={() => confirm(t('deleteConfirm')) && remove.mutate(row.original.id)}><Trash2 size={16}/></button></Tip>}{rowActions?.(row.original)}</div> })] : []),
  ], [appendOnly, canDelete, columns, editDisabled, fields.length, immutable, rowActions, t])
  const table = useReactTable({ data: rows, columns: tableColumns, getCoreRowModel: getCoreRowModel(), getFilteredRowModel: getFilteredRowModel() })
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    try {
      const values = readForm(event.currentTarget, fields)
      save.mutate({ body: normalize ? normalize(values, editing) : { ...values, ...(editing && !itemEndpoint ? { id: editing.id } : {}) }, current: editing })
    } catch { toast.error(t('invalidJson')) }
  }
  const beginCreate = (trigger: HTMLButtonElement) => { lastTrigger.current=trigger;setEditing(null);setOpen(true) }
  return <Tooltip.Provider delayDuration={400} skipDelayDuration={300}>
    <PageHeader title={title} description={description} action={!immutable && fields.length ? <button className="button button-primary" onClick={(event)=>beginCreate(event.currentTarget)}><Plus size={17}/>{createLabel || t('add')}</button> : undefined} />
    {(total > 0 || filter) && <label className="search-control"><Search size={17} aria-hidden="true"/><span className="sr-only">{t('search')}</span><input value={filter} onChange={(event) => {setFilter(event.target.value);setOffset(0)}} placeholder={t('search')} /></label>}
    {query.isError ? <QueryError retry={() => void query.refetch()} /> : query.isLoading ? <SkeletonRows /> : rows.length === 0 ? <EmptyState icon={<Inbox />} title={title} copy={filter ? t('noSearchResults') : empty} action={!filter && !immutable && fields.length ? <button className="button" onClick={(event)=>beginCreate(event.currentTarget)}>{createLabel || t('add')}</button> : undefined} /> : <><div className="table-wrap desktop-resource-table" role="region" aria-label={title} tabIndex={0}><table><thead>{table.getHeaderGroups().map((group) => <tr key={group.id}>{group.headers.map((header) => <th key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</th>)}</tr>)}</thead><tbody>{table.getRowModel().rows.map((row) => <tr key={row.id}>{row.getVisibleCells().map((cell) => <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>)}</tr>)}</tbody></table></div><MobileResources rows={rows} columns={columns} rowActions={rowActions} onEdit={!immutable&&!appendOnly&&!editDisabled&&fields.length? (row,trigger)=>{lastTrigger.current=trigger;setEditing(row);setOpen(true)}:undefined} onDelete={!immutable&&!appendOnly&&canDelete?(row)=>confirm(t('deleteConfirm'))&&remove.mutate(row.id):undefined}/></>}
    {total > limit && <nav className="pagination" aria-label={t('pagination')}><button className="button button-quiet" disabled={offset===0} onClick={()=>setOffset(Math.max(0,offset-limit))}>{t('previous')}</button><span>{offset+1}–{Math.min(offset+limit,total)} / {total}</span><button className="button button-quiet" disabled={offset+limit>=total} onClick={()=>setOffset(offset+limit)}>{t('next')}</button><select aria-label={t('pageSize')} value={limit} onChange={(event)=>{setLimit(Number(event.target.value));setOffset(0)}}><option value="10">10</option><option value="25">25</option><option value="50">50</option></select></nav>}
    <Modal open={open} onOpenChange={(value) => { setOpen(value); if (!value) {setEditing(null);requestAnimationFrame(()=>lastTrigger.current?.focus())} }} title={editing ? `${t('edit')} ${title}` : createLabel || `${t('add')} ${title}`}>
      <form className="form-stack" onSubmit={submit}>{fields.map((field) => <ResourceField key={field.key} field={field} value={editing?.[field.sourceKey || field.key] ?? field.defaultValue}/>) }<div className="form-actions"><button type="button" className="button" onClick={() => setOpen(false)}>{t('cancel')}</button><button className="button button-primary" disabled={save.isPending}>{save.isPending ? t('loading') : t('save')}</button></div></form>
    </Modal>
  </Tooltip.Provider>
}

function MobileResources({rows,columns,onEdit,onDelete,rowActions}:{rows:Document[];columns:Array<{key:string;label:string;mono?:boolean;render?:(value:unknown,row:Document)=>ReactNode}>;onEdit?: (row:Document,trigger:HTMLButtonElement)=>void;onDelete?: (row:Document)=>void;rowActions?:(row:Document)=>ReactNode}) {
  const {t}=useTranslation()
  const primary=(row:Document)=>String(row.name||row.public_name||row.email||row.pattern||row.suffix||row.id)
  const state=(row:Document)=>row.enabled!=null?<EnabledPill enabled={row.enabled}/>:displayValue(row.status)
  const visible=columns.filter(column=>column.key!=='id').slice(0,3)
  return <div className="mobile-resource-list">{rows.map(row=><article key={row.id}><header><div><strong>{primary(row)}</strong><button className="copy-id" onClick={()=>void navigator.clipboard?.writeText(String(row.id))}>{t('copyId')}</button></div><span>{state(row)}</span></header><dl>{visible.filter(column=>String(row[column.key]??'')!==primary(row)).map(column=><div key={column.key}><dt>{column.label}</dt><dd className={column.mono?'mono-cell':''}>{column.render?column.render(row[column.key],row):displayValue(row[column.key])}</dd></div>)}</dl>{(onEdit||onDelete||rowActions)&&<footer>{onEdit&&<button className="button button-quiet" onClick={(event)=>onEdit(row,event.currentTarget)}>{t('edit')}</button>}{onDelete&&<button className="button button-quiet danger" onClick={()=>onDelete(row)}>{t('delete')}</button>}{rowActions?.(row)}</footer>}</article>)}</div>
}

function ResourceField({ field, value }: { field: FormField; value: unknown }) {
  const [selected, setSelected] = useState(String(value ?? field.options?.[0]?.value ?? ''))
  if (field.kind === 'select' && field.options) return <SelectField name={field.key} label={field.label} value={selected} onValueChange={setSelected} options={field.options}/>
  if (field.kind === 'checkbox') return <label className="check-field"><input name={field.key} type="checkbox" defaultChecked={value == null ? true : Boolean(value)}/><span>{field.label}</span></label>
  const initial = field.kind === 'json' && typeof value === 'object' ? JSON.stringify(value, null, 2) : String(value ?? '')
  return <Field label={field.label} hint={field.hint}>{field.kind === 'textarea' || field.kind === 'json' ? <textarea name={field.key} required={field.required} defaultValue={initial} rows={field.kind === 'json' ? 7 : 4}/> : <input name={field.key} type={field.kind === 'secret' ? 'password' : field.kind === 'number' ? 'number' : 'text'} required={field.required} defaultValue={initial}/>}</Field>
}
