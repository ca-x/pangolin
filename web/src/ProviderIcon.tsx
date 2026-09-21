import Anthropic from '@lobehub/icons/es/Anthropic/components/Mono'
import Aws from '@lobehub/icons/es/Aws/components/Mono'
import Azure from '@lobehub/icons/es/Azure/components/Mono'
import Cloudflare from '@lobehub/icons/es/Cloudflare/components/Mono'
import DeepSeek from '@lobehub/icons/es/DeepSeek/components/Mono'
import Gemini from '@lobehub/icons/es/Gemini/components/Mono'
import Github from '@lobehub/icons/es/Github/components/Mono'
import Groq from '@lobehub/icons/es/Groq/components/Mono'
import OpenAI from '@lobehub/icons/es/OpenAI/components/Mono'
import Qwen from '@lobehub/icons/es/Qwen/components/Mono'
import SiGithubcopilot from '@icons-pack/react-simple-icons/icons/SiGithubcopilot'
import SiOllama from '@icons-pack/react-simple-icons/icons/SiOllama'
import SiX from '@icons-pack/react-simple-icons/icons/SiX'
import type { ComponentType, SVGProps } from 'react'
import { ThemeIcon } from '@mantine/core'

const lobe: Record<string, ComponentType<{ size?: string | number; title?: string }>> = { Anthropic, Aws, Azure, Cloudflare, DeepSeek, Gemini, Github, Groq, OpenAI, Qwen }
const simple: Record<string, ComponentType<SVGProps<SVGSVGElement> & { size?: string | number; title?: string }>> = { GithubCopilot: SiGithubcopilot, Ollama: SiOllama, X: SiX }

export function ProviderIcon({ logoKey, name }: { logoKey?: string; name: string }) {
  const [source, key = ''] = (logoKey || '').split(':', 2)
  const LobeIcon = source === 'lobehub' ? lobe[key] : undefined
  if (LobeIcon) return <ThemeIcon variant="light" size={32} radius="md" aria-label={name}><LobeIcon size={22} title={name} /></ThemeIcon>
  const SimpleIcon = source === 'simple' ? simple[key] : simple[key]
  if (SimpleIcon) return <ThemeIcon variant="light" size={32} radius="md" aria-label={name}><SimpleIcon size={21} title={name} /></ThemeIcon>
  const fallback = source === 'initials' && key ? key : name.split(/\s+/).map((word) => word[0]).join('').slice(0, 2)
  return <ThemeIcon variant="light" size={32} radius="md" color="pangolin" role="img" aria-label={name} style={{ fontSize: 11, fontWeight: 720 }}>{fallback.toUpperCase()}</ThemeIcon>
}