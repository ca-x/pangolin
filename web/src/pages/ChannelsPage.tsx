import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus } from 'lucide-react'
import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api, type Document, type Paged } from '../api'
import { Field, SkeletonRows } from '../components'
import { ProviderIcon } from '../ProviderIcon'
import { projectOperationPath, useProject } from '../project'
import { PageHeader, QueryError, ResourcePage } from './shared'

export default function ChannelsPage() {
  const { t } = useTranslation(); const [tab,setTab]=useState('channels')
  return <div><div className="section-tabs" role="tablist" aria-label={t('channels')}>{['channels','credentials','probes','quotas','presets'].map(value=><button key={value} role="tab" aria-selected={tab===value} onClick={()=>setTab(value)}>{t(value)}</button>)}</div>
    {tab==='channels' && <><ResourcePage resource="channels" title={t('channels')} description={t('channelsDescription')} empty={t('channelEmpty')} createLabel={t('addChannel')} columns={[{key:'id',label:'ID',mono:true},{key:'name',label:t('name'),render:(value,row)=><span className="provider-cell"><ProviderIcon logoKey={String((row.settings as Record<string,unknown>)?.logo_key||'')} name={String(value)}/><strong>{String(value)}</strong></span>},{key:'kind',label:t('providerType')},{key:'base_url',label:t('baseUrl'),mono:true},{key:'enabled',label:t('enabled'),render:(value)=>Number(value)?t('enabled'):t('disabled')}]} fields={[{key:'name',label:t('name'),required:true},{key:'kind',label:t('providerType'),kind:'select',required:true,options:providerKinds()},{key:'base_url',label:t('baseUrl'),required:true,defaultValue:'https://api.openai.com'},{key:'enabled',label:t('enabled'),kind:'checkbox'}]}/><BulkToggle/></>}
    {tab==='credentials' && <><ResourcePage resource="credentials" title={t('credentials')} description={t('credentialsDescription')} empty={t('credentialEmpty')} createLabel={t('addCredential')} columns={[{key:'id',label:'ID',mono:true},{key:'provider_id',label:t('channel'),mono:true},{key:'credential_type',label:t('credentialType')},{key:'suffix',label:t('suffix'),mono:true},{key:'priority',label:t('priority')},{key:'enabled',label:t('enabled'),render:(value)=>Number(value)?t('enabled'):t('disabled')}]} fields={[{key:'provider_id',label:t('channelId'),required:true},{key:'credential_type',label:t('credentialType'),defaultValue:'api_key',required:true},{key:'secret',label:t('secret'),kind:'secret',hint:t('secretUpdateHint')},{key:'priority',label:t('priority'),kind:'number',defaultValue:100},{key:'enabled',label:t('enabled'),kind:'checkbox'},{key:'settings',label:t('advancedSettings'),kind:'json',defaultValue:{version:1}}]}/><BulkToggle resource="credentials"/></>}
    {tab==='probes' && <><Diagnostics/><ResourcePage resource="probes" title={t('probes')} description={t('probesDescription')} empty={t('probeEmpty')} immutable columns={[{key:'provider_id',label:t('channel'),mono:true},{key:'success',label:t('status')},{key:'status_code',label:t('statusCode')},{key:'latency_ms',label:t('latency')},{key:'probed_at',label:t('time')}]} /></>}
    {tab==='quotas' && <><Diagnostics quota/><ResourcePage resource="quotas" title={t('quotas')} description={t('quotasDescription')} empty={t('quotaEmpty')} immutable columns={[{key:'provider_id',label:t('channel'),mono:true},{key:'remaining_micros',label:t('remaining')},{key:'period_end',label:t('periodEnd')},{key:'collected_at',label:t('time')}]} /></>}
    {tab==='presets' && <PresetsPanel/>}
  </div>
}

function providerKinds(){return ['openai','openai_compatible','anthropic','gemini','azure','bedrock','vertex','gcp','openrouter','deepseek','moonshot','zhipu','doubao','xai','groq','ollama','nanogpt','jina'].map(value=>({value,label:value.replaceAll('_',' ')}))}

function BulkToggle({resource='channels'}:{resource?:'channels'|'credentials'}) {
  const {t}=useTranslation(); const {project}=useProject(); const client=useQueryClient()
  const mutate=useMutation({mutationFn:(body:unknown)=>api(projectOperationPath(project.id,'bulk-toggle'),{method:'POST',body:JSON.stringify(body)}),onSuccess:()=>{toast.success(t('saved'));void client.invalidateQueries({queryKey:['resource',project.id,resource]})},onError:(error:Error)=>toast.error(error.message)})
  const submit=(event:FormEvent<HTMLFormElement>)=>{event.preventDefault();const data=new FormData(event.currentTarget);mutate.mutate({resource,ids:String(data.get('ids')).split(',').map(id=>id.trim()).filter(Boolean),enabled:data.get('enabled')==='true'})}
  return <details className="inline-tool"><summary>{t('bulkActions')}</summary><form className="form-row" onSubmit={submit}><Field label={t('resourceIds')}><input name="ids" required placeholder="id-1, id-2"/></Field><label className="field"><span className="field-label">{t('action')}</span><select name="enabled" defaultValue="true"><option value="true">{t('enable')}</option><option value="false">{t('disable')}</option></select></label><button className="button">{t('apply')}</button></form></details>
}

function Diagnostics({quota=false}:{quota?:boolean}) {
  const {t}=useTranslation(); const {project}=useProject(); const run=useMutation({mutationFn:(provider_id:string)=>api(projectOperationPath(project.id,quota?'quota':'probe'),{method:'POST',body:JSON.stringify({provider_id})}),onSuccess:()=>toast.success(t('jobQueued')),onError:(error:Error)=>toast.error(error.message)})
  const submit=(event:FormEvent<HTMLFormElement>)=>{event.preventDefault();run.mutate(String(new FormData(event.currentTarget).get('provider_id')))}
  return <form className="diagnostic-bar" onSubmit={submit}><Field label={t('channelId')}><input name="provider_id" required/></Field><button className="button button-primary">{quota?t('collectQuota'):t('runProbe')}</button></form>
}

function PresetsPanel() {
  const {t}=useTranslation(); const {project}=useProject(); const client=useQueryClient()
  const query=useQuery({queryKey:['catalog-providers'],queryFn:()=>api<Paged<Document>>('/api/admin/v1/catalog/providers?limit=200')})
  const create=useMutation({mutationFn:(preset:Document)=>api(projectOperationPath(project.id,'channels'),{method:'POST',body:JSON.stringify({name:preset.name,kind:preset.adapter_kind,base_url:preset.default_base_url,enabled:true,settings:{version:1,catalog_provider_id:preset.id,logo_key:preset.logo_key}})}),onSuccess:()=>{toast.success(t('saved'));void client.invalidateQueries({queryKey:['resource',project.id,'channels']})},onError:(error:Error)=>toast.error(error.message)})
  if(query.isError)return <QueryError retry={()=>void query.refetch()}/>;if(query.isLoading)return <SkeletonRows/>;return <><PageHeader title={t('presets')} description={t('presetsDescription')}/><div className="preset-grid">{(query.data?.data||[]).map(preset=><article key={preset.id}><ProviderIcon logoKey={String(preset.logo_key||'')} name={String(preset.name)}/><div><h2>{String(preset.name)}</h2><code>{String(preset.default_base_url||'—')}</code></div><button className="button" disabled={!preset.adapter_available||!preset.adapter_kind||!preset.default_base_url||create.isPending} onClick={()=>create.mutate(preset)}><Plus size={16}/>{preset.adapter_available?t('usePreset'):t('catalogOnly')}</button></article>)}</div></>
}
