import Anthropic from '@lobehub/icons/es/Anthropic/components/Mono'
import Aws from '@lobehub/icons/es/Aws/components/Mono'
import Azure from '@lobehub/icons/es/Azure/components/Mono'
import Cloudflare from '@lobehub/icons/es/Cloudflare/components/Mono'
import DeepSeek from '@lobehub/icons/es/DeepSeek/components/Mono'
import Doubao from '@lobehub/icons/es/Doubao/components/Mono'
import Gemini from '@lobehub/icons/es/Gemini/components/Mono'
import Github from '@lobehub/icons/es/Github/components/Mono'
import GoogleCloud from '@lobehub/icons/es/GoogleCloud/components/Mono'
import Groq from '@lobehub/icons/es/Groq/components/Mono'
import Jina from '@lobehub/icons/es/Jina/components/Mono'
import Moonshot from '@lobehub/icons/es/Moonshot/components/Mono'
import OpenAI from '@lobehub/icons/es/OpenAI/components/Mono'
import OpenRouter from '@lobehub/icons/es/OpenRouter/components/Mono'
import Qwen from '@lobehub/icons/es/Qwen/components/Mono'
import VertexAI from '@lobehub/icons/es/VertexAI/components/Mono'
import XAI from '@lobehub/icons/es/XAI/components/Mono'
import Zhipu from '@lobehub/icons/es/Zhipu/components/Mono'
import SiGithubcopilot from '@icons-pack/react-simple-icons/icons/SiGithubcopilot'
import SiOllama from '@icons-pack/react-simple-icons/icons/SiOllama'
import SiX from '@icons-pack/react-simple-icons/icons/SiX'
import type { ComponentType, SVGProps } from 'react'
import { ThemeIcon } from '@mantine/core'

const lobe: Record<string, ComponentType<{ size?: string | number; title?: string }>> = { Anthropic, Aws, Azure, Cloudflare, DeepSeek, Doubao, Gemini, Github, GoogleCloud, Groq, Jina, Moonshot, OpenAI, OpenRouter, Qwen, VertexAI, XAI, Zhipu }
const simple: Record<string, ComponentType<SVGProps<SVGSVGElement> & { size?: string | number; title?: string }>> = { GithubCopilot: SiGithubcopilot, Ollama: SiOllama, X: SiX }

export function ProviderIcon({ logoKey, name, size = 32 }: { logoKey?: string; name: string; size?: number }) {
  const [source, key = ''] = (logoKey || '').split(':', 2)
  // The badge scales with the requested size: operational tables use 32px, the
  // provider picker uses the reference product's ~20px icon. At 32 the numbers
  // below are the original 22/21/11, so existing call sites render unchanged.
  const inner = Math.round(size * 0.69)
  const LobeIcon = source === 'lobehub' ? lobe[key] : undefined
  if (LobeIcon) return <ThemeIcon variant="light" size={size} radius="md" aria-label={name}><LobeIcon size={inner} title={name} /></ThemeIcon>
  const SimpleIcon = source === 'simple' ? simple[key] : simple[key]
  if (SimpleIcon) return <ThemeIcon variant="light" size={size} radius="md" aria-label={name}><SimpleIcon size={inner - 1} title={name} /></ThemeIcon>
  const fallback = source === 'initials' && key ? key : name.split(/\s+/).map((word) => word[0]).join('').slice(0, 2)
  return <ThemeIcon variant="light" size={size} radius="md" color="pangolin" role="img" aria-label={name} style={{ fontSize: Math.max(9, Math.round(size * 0.34)), fontWeight: 720 }}>{fallback.toUpperCase()}</ThemeIcon>
}