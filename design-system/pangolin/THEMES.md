# Pangolin 主题系统 / Theme system

Sources of truth: this file for the theme axes, [`MASTER.md`](MASTER.md) for the
house design system, `web/src/skins.ts` / `web/src/palettes.ts` for the data,
`web/src/mantine.ts` for how the two become a component-library theme.

## 三个正交轴 / Three orthogonal axes

| 轴 | 取值 | 决定什么 | 数据位置 |
| --- | --- | --- | --- |
| 明暗 `data-mode` | `light` / `dark`（`system` 解析后） | 表面与文字的明暗 | `palettes.ts` 每套配色的两个模式 |
| 风格 `data-skin` | 16（15 个风格库主题 + 鲮鲤默认） | 几何、层次、字体、材质、动效性格、密度 | `skins.ts` |
| 配色 `data-palette` | 20（17 套风格库配色 + 铜棕/石板蓝/玉青） | 颜色 | `palettes.ts` |

风格与配色相互独立：换风格会**同时套用它的推荐配色与推荐模式**（多数风格只有一种模式成立，见下），之后仍可单独改配色。

## 来源与选择标准 / Provenance

风格取自 `ui-ux-pro-max` 风格库的 50 个 active 条目；配色取自其 192 套产品配色的 17 套。

**筛除标准**：能否作为**令牌/材质层**实现。凡身份即"布局范式"或"交互范式"的风格一律不取——Bento Grid、Parallax Storytelling、Kinetic Typography、Interactive Cursor、Voice-First、3D Product Preview、Spatial UI、Motion-Driven、Biomimetic、Tactile Deformable、Pixel Art（像素字体不适合密集正文）、Gen Z Chaos、Liquid Glass（与磨砂玻璃重复）。剩下 15 个。

**已知偏离（重要）**：风格库对其中多个明确标注"不适用于数据密集仪表盘"——Neumorphism（`risk:high`）、Editorial Grid、Aurora、Claymorphism、Vibrant & Block-based 均如此，Skeuomorphism 甚至写"现代企业请用 Flat"。Pangolin 是密集控制台，因此每个皮肤**只取该风格的视觉语言**，并强制保留本项目的底线：

- 表格正文 **≥14px**，行高 **≥44px**（风格库建议 12px / 36px 的部分不予采纳）；
- 正文对比度 **≥4.5:1**、控件边界 **≥3:1**（WCAG 1.4.11）；
- 表现性效果只允许出现在 chrome 与非数据表面，**绝不垫在表格数据下面**；
- 全部动效 <300ms 且只动 `transform`/`opacity`。

| 皮肤 | 风格库条目 | 材质 | 模式 | 推荐配色 |
| --- | --- | --- | --- | --- |
| `house` | —（鲮鲤默认） | flat | auto | 铜棕 |
| `swiss` | Minimalism & Swiss Style | flat | auto | 传统银行 |
| `paper` | E-Ink / Paper | paper | 仅浅色 | 账单与发票 |
| `dense` | Data-Dense Dashboard | flat | auto | 开发者工具 |
| `accessible` | Accessible & Ethical | flat | auto | 传统银行 |
| `fluent` | Fluent 2 | soft | auto | 通用 SaaS |
| `glass` | Glassmorphism | glass | 仅浅色 | 生物科技 |
| `aurora` | Aurora UI | aurora | 仅深色 | AI 对话平台 |
| `skeuo` | Skeuomorphism | bevel | 仅浅色 | 铜棕 |
| `neumorph` | Neumorphism | soft | 仅浅色 | 效率工具 |
| `clay` | Claymorphism | clay | 仅浅色 | 宠物科技 |
| `vibrant` | Vibrant & Block-based | block | 仅浅色 | 游戏 |
| `y2k` | Y2K Aesthetic | chrome | auto | 航天科技 |
| `hud` | HUD / Sci-Fi FUI | hud | 仅深色 | 网络安全 |
| `oled` | Dark Mode (OLED) | flat | 仅深色 | API 门户 |
| `editorial` | Editorial Grid / Magazine | paper | 仅浅色 | 账单与发票 |

## 配色与推导 / Palettes and derivation

风格库每套配色只描述**一种模式**。同模式按原样使用；另一半模式由 `web/src/color.ts` 推导：**保持色相**，用 OKLCH 亮度锚点重建中性色阶，彩度夹入 sRGB 色域（超域时逐步降彩度而不是让通道截断，避免色相漂移）。深色推导出的中性色带一丝强调色相，以免看起来像另一个产品。

## 对比度是构造性保证，不是事后检查 / Contrast by construction

`web/src/palettes.test.ts` 对 20 套配色 × 2 模式 × 23 项配对做断言，共 **922 项**。它不依赖推导"碰巧合格"：`color.ts` 的 `ensureContrast()` 只在**未达标**时把颜色沿 OKLCH 亮度轴移到刚好达标的位置，已达标的一律保持作者原样。测试用于验证这一保证。

审计第一次运行时暴露了**现有默认主题的真实缺陷**，均已修复：

| 问题 | 实测 | 要求 | 处理 |
| --- | --- | --- | --- |
| `--control-border` 对画布/表面 | 1.56:1 / 1.80:1 | ≥3:1（WCAG 1.4.11） | 控件边界提到达标（装饰性分隔线 `--border` 不受此约束） |
| `--warning` 对画布 | 4.47:1 | ≥4.5:1 | 微调到达标 |
| OLED 类配色 `--border` 对表面 | 1.13:1 | ≥1.15:1（可辨识） | 轻微提亮 |

## 实现 / Implementation

- **组件来自 Mantine 9**（`@mantine/core`），主题由 `buildTheme(skin, palette, mode)` 生成：色阶（第 6 级=浅色强调、第 4 级=深色强调，对应 `primaryShade`）、圆角、阴影、字体、字号、间距、各组件默认值。
- **调色板 → Mantine 变量**：`buildResolver()` 把配色的表面/文字/边框/强调映射到 `--mantine-*`。这些变量必须同时写进 light 与 dark 两个区块——Mantine 把它们定义在 `[data-mantine-color-scheme]` 下，优先级高于 `:root`。
- **材质层** `web/src/materials.css` 只保留组件库无法表达的部分（磨砂、黏土、金属、HUD 网格、纸张颗粒、极光场）。主题通过 `pm-*` 类名把材质挂到组件上，**不依赖 Mantine 的哈希类名**。
- 未迁移的旧 CSS 仍读取 `<html>` 上的内联令牌（`theme.tsx` 写入），因此两套实现始终同色，迁移可以逐文件进行。

## 无障碍门槛 / Accessibility gates

每个材质都必须响应 `prefers-reduced-transparency`（转为实心、去模糊）、`prefers-reduced-motion`（去位移与动画）、`prefers-contrast: more`（实心 + 定义边框）。焦点环在所有皮肤可见；`accessible` 皮肤使用 3px 焦点环与 `focusRing: 'always'`。

## 新增一个皮肤或配色 / Adding one

1. 配色：往 `palettes.ts` 的 `catalog` 加一行（保留风格库原始字段），无需写 CSS。
2. 皮肤：往 `skins.ts` 加一条（圆角/阴影/字体/密度/材质/动效/推荐配色），在 `skins.test.ts` 的标签表里补中英文名；若引入新材质，在 `materials.css` 里加 `[data-material='新材质']` 规则（`skins.test.ts` 会检查是否漏写）。
3. 跑 `pnpm test`：一致性、对比度与令牌格式都会被自动验证。
