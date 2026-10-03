import { Button, Checkbox, Group, Select, Stack, Text, Textarea, TextInput } from '@mantine/core'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type Document } from '../api'
import { Modal, SkeletonRows } from '../components'
import { projectOperationPath, useProject } from '../project'
import { QueryError } from './shared'
import { utf8Size } from './ProtectionRequestPreview'
export function validAllowlist(value: string): boolean {
  try {
    const list: unknown = JSON.parse(value)
    return Array.isArray(list) && list.length <= 64 && list.every((v) => typeof v === 'string' && utf8Size(v) <= 256)
  } catch {
    return false
  }
}
export function AllowlistField({
  name,
  label,
  value,
  setValid,
}: {
  name: string
  label: string
  value: unknown
  setValid: (valid: boolean) => void
}) {
  const { t } = useTranslation()
  const [text, setText] = useState(() => JSON.stringify(Array.isArray(value) ? value : [], null, 2))
  const valid = validAllowlist(text)
  useEffect(() => setValid(valid), [valid, setValid])
  return (
    <Textarea
      name={name}
      label={label}
      description={t('allowlistHelp')}
      value={text}
      onChange={(event) => {
        setText(event.currentTarget.value)
        setValid(validAllowlist(event.currentTarget.value))
      }}
      error={!valid ? t('allowlistError') : undefined}
      rows={4}
      styles={{ input: { fontFamily: 'var(--font-mono)', fontSize: 14 } }}
    />
  )
}
export function ProtectionTemplates() {
  const { project } = useProject()
  return <TemplatePicker key={project.id} projectId={project.id} />
}
function TemplatePicker({ projectId }: { projectId: string }) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [selected, setSelected] = useState<string | null>(null)
  const [allowlist, setAllowlist] = useState('[]')
  const [error, setError] = useState(false)
  const templates = useQuery({
    queryKey: ['protection-templates', projectId],
    queryFn: () => api<{ templates: Document[] }>(projectOperationPath(projectId, 'protection-templates')),
    enabled: open,
  })
  const known = (templates.data?.templates ?? []).filter((v) =>
    ['email', 'phone', 'api_key'].includes(String(v.template_id)),
  )
  const chosen = known.find((v) => v.template_id === selected)
  const save = useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      api(projectOperationPath(projectId, 'protection'), { method: 'POST', body: JSON.stringify(body) }),
    onSuccess: () => {
      setOpen(false)
      setSelected(null)
      setAllowlist('[]')
      void client.invalidateQueries({ queryKey: ['resource', projectId, 'protection'] })
    },
    onError: () => setError(true),
  })
  return (
    <>
      <Button variant="default" onClick={() => setOpen(true)}>
        {t('protectionTemplates')}
      </Button>
      <Modal
        open={open}
        onOpenChange={(value) => {
          setOpen(value)
          if (!value) {
            setSelected(null)
            setAllowlist('[]')
            setError(false)
            save.reset()
          }
        }}
        title={t('protectionTemplates')}
      >
        <Stack gap="md">
          <Text size="sm" c="dimmed">
            {t('templateHelp')}
          </Text>
          {templates.isError ? (
            <QueryError retry={() => void templates.refetch()} />
          ) : templates.isLoading ? (
            <SkeletonRows count={2} />
          ) : !known.length ? (
            <Text size="sm">{t('templateEmpty')}</Text>
          ) : (
            <Select
              label={t('protectionTemplate')}
              data={known.map((v) => ({ value: String(v.template_id), label: t(`template_${v.template_id}`) }))}
              value={selected}
              onChange={(value) => {
                setSelected(value)
                setAllowlist('[]')
              }}
            />
          )}
          {chosen && (
            <form
              key={selected}
              onSubmit={(event) => {
                event.preventDefault()
                if (!validAllowlist(allowlist)) return
                const form = new FormData(event.currentTarget)
                setError(false)
                save.mutate({
                  name: String(form.get('name')),
                  description: String(form.get('description')),
                  content_pattern: String(form.get('content_pattern')),
                  action: 'redact',
                  replacement: String(form.get('replacement')),
                  allowlist: JSON.parse(allowlist),
                  enabled: form.get('enabled') === 'on',
                  test_mode: false,
                  state: 'active',
                  scopes: { version: 1 },
                })
              }}
            >
              <Stack gap="md">
                <TextInput name="name" label={t('name')} required defaultValue={t(`template_${selected}`)} />
                <Textarea name="description" label={t('description')} defaultValue={String(chosen.description ?? '')} />
                <Textarea
                  name="content_pattern"
                  label={t('contentPattern')}
                  required
                  defaultValue={String(chosen.content_pattern)}
                />
                <TextInput
                  name="replacement"
                  label={t('replacement')}
                  required
                  defaultValue={String(chosen.replacement)}
                />
                <Textarea
                  label={t('allowlistLabel')}
                  description={t('allowlistHelp')}
                  value={allowlist}
                  onChange={(event) => setAllowlist(event.currentTarget.value)}
                  error={!validAllowlist(allowlist) ? t('allowlistError') : undefined}
                />
                <Checkbox name="enabled" label={t('enabled')} defaultChecked={false} />
                {error && (
                  <Text role="alert" size="sm" c="red">
                    {t('networkError')}
                  </Text>
                )}
                <Group justify="flex-end">
                  <Button type="submit" loading={save.isPending} disabled={!validAllowlist(allowlist)}>
                    {t('templateSave')}
                  </Button>
                </Group>
              </Stack>
            </form>
          )}
        </Stack>
      </Modal>
    </>
  )
}
