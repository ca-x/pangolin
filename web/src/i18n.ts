import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

const zh = {
  translation: {
    product: '鲮鲤', productSubtitle: 'AI API 网关', overview: '概览', providers: '供应商', models: '模型与路由', apiKeys: 'API 密钥', requests: '请求记录', settings: '设置',
    signOut: '退出登录', menu: '菜单', close: '关闭', skipToContent: '跳到主要内容', dataTable: '数据表', prefix: '前缀', trace: '追踪 ID', requestPayload: '请求正文', responsePayload: '响应正文', add: '添加', cancel: '取消', save: '保存', delete: '删除', copy: '复制', copied: '已复制', loading: '加载中…', retry: '重试', noData: '暂无数据',
    setupTitle: '初始化鲮鲤', setupIntro: '创建首位管理员。完成后即可添加上游供应商并签发虚拟 API 密钥。', instanceName: '实例名称', email: '邮箱', password: '密码', passwordHint: '至少 12 个字符', createAdmin: '创建管理员',
    loginTitle: '登录鲮鲤', loginIntro: '进入你的 AI API 控制平面。', signIn: '登录',
    last24h: '过去 24 小时', totalRequests: '请求总量', errorRate: '错误率', p95Latency: 'P95 延迟', tokenUsage: 'Token 用量', cost: '成本', requestTrend: '请求趋势', recentRequests: '最近请求',
    providerName: '供应商名称', providerType: '类型', baseUrl: 'Base URL', upstreamApiKey: '上游 API Key', addProvider: '添加供应商', providerEmpty: '先添加一个上游供应商。密钥将加密保存在本机。',
    publicModel: '公开模型名', upstreamModel: '上游模型名', provider: '供应商', priority: '优先级', inputPrice: '输入价格', outputPrice: '输出价格', addModel: '添加模型', priceHint: '微美元 / 百万 Token', modelEmpty: '添加模型映射后，客户端即可使用统一模型名调用。',
    keyName: '密钥名称', budget: '预算', addKey: '创建 API 密钥', keyCreated: 'API 密钥已创建', keyCreatedHint: '它只显示一次，请立即复制并安全保存。', keyEmpty: '为客户端或团队创建一把虚拟密钥。', lastUsed: '最后使用', never: '从未',
    status: '状态', endpoint: '端点', startedAt: '开始时间', latency: '延迟', model: '模型', tokens: 'Token', requestDetail: '请求详情', payloadDisabled: '敏感负载采集已关闭', payloadDisabledHint: '这是默认的安全设置。可观测性仍会记录元数据、用量和延迟。',
    appearance: '外观', colorMode: '明暗模式', accentTheme: '强调色', language: '语言', system: '跟随系统', light: '浅色', dark: '深色', bronze: '铜棕', slate: '石板蓝', jade: '玉青',
    securityDefaults: '安全默认值', securityCopy: '上游密钥采用本机主密钥加密；虚拟密钥和密码仅保存哈希；请求与响应正文默认不落盘。',
    deleteConfirm: '确认删除？此操作无法撤销。', formError: '请检查表单内容后重试。', networkError: '请求失败，请稍后重试。', healthy: '正常', degraded: '异常',
  },
}

const en = {
  translation: {
    product: 'Pangolin', productSubtitle: 'AI API gateway', overview: 'Overview', providers: 'Providers', models: 'Models & routes', apiKeys: 'API keys', requests: 'Requests', settings: 'Settings',
    signOut: 'Sign out', menu: 'Menu', close: 'Close', skipToContent: 'Skip to main content', dataTable: 'Data table', prefix: 'Prefix', trace: 'Trace ID', requestPayload: 'Request payload', responsePayload: 'Response payload', add: 'Add', cancel: 'Cancel', save: 'Save', delete: 'Delete', copy: 'Copy', copied: 'Copied', loading: 'Loading…', retry: 'Retry', noData: 'No data yet',
    setupTitle: 'Initialize Pangolin', setupIntro: 'Create the first administrator. Then add an upstream provider and issue virtual API keys.', instanceName: 'Instance name', email: 'Email', password: 'Password', passwordHint: 'At least 12 characters', createAdmin: 'Create administrator',
    loginTitle: 'Sign in to Pangolin', loginIntro: 'Enter your AI API control plane.', signIn: 'Sign in',
    last24h: 'Last 24 hours', totalRequests: 'Requests', errorRate: 'Error rate', p95Latency: 'P95 latency', tokenUsage: 'Token usage', cost: 'Cost', requestTrend: 'Request trend', recentRequests: 'Recent requests',
    providerName: 'Provider name', providerType: 'Type', baseUrl: 'Base URL', upstreamApiKey: 'Upstream API key', addProvider: 'Add provider', providerEmpty: 'Add an upstream provider first. Its key is encrypted on this machine.',
    publicModel: 'Public model name', upstreamModel: 'Upstream model', provider: 'Provider', priority: 'Priority', inputPrice: 'Input price', outputPrice: 'Output price', addModel: 'Add model', priceHint: 'micro-USD / million tokens', modelEmpty: 'Map a model so clients can call a stable public name.',
    keyName: 'Key name', budget: 'Budget', addKey: 'Create API key', keyCreated: 'API key created', keyCreatedHint: 'This is shown once. Copy and store it securely now.', keyEmpty: 'Create a virtual key for a client or team.', lastUsed: 'Last used', never: 'Never',
    status: 'Status', endpoint: 'Endpoint', startedAt: 'Started', latency: 'Latency', model: 'Model', tokens: 'Tokens', requestDetail: 'Request detail', payloadDisabled: 'Sensitive payload capture is off', payloadDisabledHint: 'This is the secure default. Metadata, usage and latency are still recorded.',
    appearance: 'Appearance', colorMode: 'Color mode', accentTheme: 'Accent theme', language: 'Language', system: 'System', light: 'Light', dark: 'Dark', bronze: 'Bronze', slate: 'Slate', jade: 'Jade',
    securityDefaults: 'Security defaults', securityCopy: 'Upstream secrets are encrypted by a local master key; virtual keys and passwords are hashed; request and response bodies are not stored by default.',
    deleteConfirm: 'Delete this item? This cannot be undone.', formError: 'Check the form and try again.', networkError: 'The request failed. Try again shortly.', healthy: 'Healthy', degraded: 'Degraded',
  },
}

const storedLanguage = localStorage.getItem('pangolin-language') || (navigator.language.startsWith('zh') ? 'zh-CN' : 'en')

void i18n.use(initReactI18next).init({ resources: { 'zh-CN': zh, en }, lng: storedLanguage, fallbackLng: 'en', interpolation: { escapeValue: false } })

i18n.on('languageChanged', (language) => {
  localStorage.setItem('pangolin-language', language)
  document.documentElement.lang = language
})

export default i18n
