import { QueryClient,QueryClientProvider } from '@tanstack/react-query'
import { render,screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { beforeEach,describe,expect,it,vi } from 'vitest'
import i18n from '../i18n'
import { ProjectProvider } from '../project'
import { ResourcePage } from './shared'

const json=(value:unknown)=>Promise.resolve(new Response(JSON.stringify(value),{status:200,headers:{'Content-Type':'application/json'}}))
describe('resource management contracts',()=>{
  beforeEach(()=>{void i18n.changeLanguage('en')})
  it('omits blank secrets, paginates on the server, and restores dialog focus',async()=>{
    const fetchMock=vi.fn((input:RequestInfo|URL,init?:RequestInit)=>{const path=String(input);if(path.endsWith('/projects'))return json([{id:'p1',name:'Project',slug:'project',owner_user_id:'u1',is_default:true,enabled:true}]);if(path.includes('/permissions'))return json(['*']);if(init?.method==='POST')return json({id:'s1'});return json({data:[{id:'s1',name:'Primary',secret_configured:true}],total:30,offset:path.includes('offset=25')?25:0,limit:25})});vi.stubGlobal('fetch',fetchMock)
    const client=new QueryClient({defaultOptions:{queries:{retry:false}}});render(<QueryClientProvider client={client}><MemoryRouter><ProjectProvider><ResourcePage resource="storage" title="Storage" description="Targets" empty="None" columns={[{key:'name',label:'Name'}]} fields={[{key:'name',label:'Name',required:true},{key:'secret',label:'Secret',kind:'secret',omitWhenBlank:true}]}/></ProjectProvider></MemoryRouter></QueryClientProvider>)
    const edit=(await screen.findAllByRole('button',{name:'Edit'}))[0];await userEvent.click(edit);await userEvent.click(screen.getByRole('button',{name:'Save'}));await vi.waitFor(()=>{const call=fetchMock.mock.calls.find(([,init])=>init?.method==='POST');expect(call).toBeTruthy();expect(JSON.parse(String(call?.[1]?.body))).toEqual({id:'s1',name:'Primary'})});await vi.waitFor(()=>expect(edit).toHaveFocus())
    await userEvent.click(edit);await userEvent.keyboard('{Escape}');await vi.waitFor(()=>expect(edit).toHaveFocus())
    await userEvent.click(screen.getByRole('button',{name:'Next'}));await vi.waitFor(()=>expect(fetchMock.mock.calls.some(([path])=>String(path).includes('offset=25')&&String(path).includes('limit=25'))).toBe(true))
  })
})
