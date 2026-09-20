import {QueryClient,QueryClientProvider} from '@tanstack/react-query'
import {render,screen} from '@testing-library/react'
import {MemoryRouter,Route,Routes} from 'react-router'
import {describe,expect,it,vi} from 'vitest'
import {ProjectProvider} from '../project'
import '../i18n'
import {TraceDetailPage} from './OperationsPage'
const response=(value:unknown)=>Promise.resolve(new Response(JSON.stringify(value),{status:200,headers:{'Content-Type':'application/json'}}))
describe('trace navigation',()=>{it('links to the public request identifier',async()=>{vi.stubGlobal('fetch',vi.fn((input:RequestInfo|URL)=>{const path=String(input);if(path.endsWith('/projects'))return response([{id:'p1',name:'P',slug:'p',owner_user_id:'u',is_default:true,enabled:true}]);if(path.includes('/permissions'))return response(['*']);return response({trace:{id:'trace-1',status:'succeeded',started_at:1,finished_at:2,thread_id:null},requests:[{id:'internal-id',public_id:'public-id',status:'succeeded',endpoint:'/v1/chat/completions',model:'demo',started_at:1,finished_at:2}],executions:[]})}));const client=new QueryClient({defaultOptions:{queries:{retry:false}}});render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/operations/traces/trace-1']}><ProjectProvider><Routes><Route path="/operations/traces/:id" element={<TraceDetailPage/>}/></Routes></ProjectProvider></MemoryRouter></QueryClientProvider>);expect(await screen.findByRole('link',{name:'View details'})).toHaveAttribute('href','/operations/requests/public-id')})})
