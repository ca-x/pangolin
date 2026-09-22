# Pangolin 前端 Refactoring UI 审核

> 审核日期：2026-09-21  
> 审核对象：当前工作树重新构建的 release binary，默认 `Pangolin house + Bronze` 主题  
> 审核方法：以《Refactoring UI》的视觉层级、间距、排版、色彩、层次和空状态规则为主，同时核对 `design-system/pangolin/MASTER.md` 与仓库前端约束。

## 审核范围

- 视口：375×812、768×900、1440×1000。
- 模式：浅色、深色。
- 语言：English、简体中文。
- 页面：登录、概览、渠道、模型与路由、提示词、调试台、运行记录、访问控制、系统设置、模型目录、添加渠道弹窗和移动端导航抽屉。
- 状态：首次配置、空状态、内置模型目录的有数据状态。
- 自动检查：axe WCAG 2 A/AA、页面/滚动区域尺寸、关键字号与颜色对比度。

本次没有遍历全部 15 套界面风格和 20 套配色，也没有用真实请求数据覆盖运行记录表格；结论针对默认 house 风格，并优先记录可稳定复现的问题。

## 总体结论

整体基础是可靠的：主内容层级清楚，桌面表格的 54px 行高和 14px 正文达到设计源要求；375px 下没有页面级横向滚动，数据表也确实切换成了卡片；中英文与浅/深色切换没有发现布局崩溃。

当前主要短板不在“是否好看”，而在体系没有完全落到组件上：深色主按钮没有使用规定的对比色；首次配置提示、页面主操作和空状态操作互相争抢；移动端抽屉未完全离屏；有数据的模型目录又把 9px 胶囊标签和 25 条长列表带回来了。按照发布风险排序，共记录 9 项：高优先级 2 项，中优先级 7 项。

## 问题清单

### P1-1 深色模式主按钮文字对比度系统性不达标

**严重度：高**  
**范围：** 深色模式下的概览、渠道、系统设置、弹窗等主按钮。

实际按钮仍使用白色文字：

- `#ffffff` / `#d78f6b`：`2.61:1`，影响“添加渠道”“应用”“保存”等 14px 按钮。
- `#f3eeea` / `#d78f6b`：`2.26:1`，影响概览 12.5px 的“配置渠道”。

两者都低于 WCAG AA 的 `4.5:1`。这也直接违背设计源中深色模式 `--accent-contrast: #1b1210` 的规定。问题不是单个页面调色失误，而是主按钮组件没有统一消费 accent contrast token。

**建议：** 所有 primary button 的前景色统一改为 `var(--accent-contrast)`，禁止组件库默认白字覆盖；对 bronze/slate/jade × light/dark 做参数化对比度测试，并把 axe 检查纳入回归。

### P1-2 弹窗关闭按钮没有可访问名称

**严重度：高**  
**范围：** 375px 深色中文的“添加渠道”弹窗，其他同源弹窗应一并检查。

axe 将关闭按钮报为 `button-name` critical：按钮无可读文字、`aria-label`、`aria-labelledby` 或 `title`。视觉用户能看到 `×`，屏幕阅读器用户却无法判断按钮作用，违反仓库“图标按钮必须有 accessible name”的硬约束。

**建议：** 在共享 Modal/CloseButton 层补本地化名称，而不是逐弹窗修补；中文为“关闭”，英文为“Close”。增加打开弹窗后按名称查找关闭按钮的自动化测试。

### P2-1 关闭状态的移动抽屉仍露出 6px 边框

**严重度：中**  
**范围：** 375px 与 768px 的所有页面、两种主题。

关闭后的导航节点实测边界为 `x=-369, width=375`，右边缘仍停在 `x=6`。因此页面左侧从顶栏以下持续露出一条带圆角的边框/阴影，深色和浅色都清晰可见。它看起来像裁切错误，也破坏了主内容左右留白。

**建议：** 关闭态保证整个 chrome（含 border/shadow）移出视口，或在退出完成后切换 `visibility: hidden`；断言关闭态右边缘 `<= 0`。不要只移动内容盒而留下外描边。

### P2-2 打开移动导航后出现两套品牌头和两个同级关闭按钮

**严重度：中**  
**范围：** 375px 移动导航。

抽屉从顶栏下方开始，但内部再次放置 logo、品牌名和关闭按钮；外层顶栏仍保留同一套品牌和另一个关闭按钮。用户同时看到两个 `×`，无法从层级判断哪个控制抽屉；重复 chrome 也浪费约 60px 垂直空间。

**建议：** 二选一：让抽屉覆盖完整视口并只保留抽屉头，或让抽屉从顶栏下方开始但删除内部品牌头/关闭按钮。关闭操作只保留一个明确入口。

### P2-3 首次配置提示与页面主操作重复竞争

**严重度：中**  
**范围：** 未完成 onboarding 时的所有控制台页面，渠道页最明显。

顶部“完成首次配置”横幅跨越所有页面并带“配置渠道”操作；渠道页标题区另有实心“添加渠道”，空状态中又有一次“添加渠道”。同一屏最多出现 3 个等价入口。横幅即使在系统设置、访问控制和运行记录页也持续占据最高位置，压过当前任务标题。

这命中 Refactoring UI 的“每页通常只有一个主要操作”和“通过弱化竞争元素来强调”原则。

**建议：** onboarding 提示只在概览页完整展示；其他页降级为可关闭的紧凑提示或不展示。资源页面保留一个实心主按钮；空状态 CTA 与标题区按钮二选一，或将其中一个降为文本链接。

### P2-4 空状态仍展示无效过滤器、暂停和批量操作

**严重度：中**  
**范围：** 渠道、运行记录、访问控制等空状态页面。

- 没有渠道时仍显示“批量启用或停用”表单。
- 没有请求时仍显示状态码/供应商/模型过滤器和“暂停”。
- API key 为空时仍展示“按密钥请求日志”折叠区。

这些控件此时不能帮助用户完成首要任务，反而让空状态与正常数据页的层级混在一起。Refactoring UI 明确要求空状态隐藏标签页、过滤器等无用操作，并给出清晰行动号召。

**建议：** 数据为零时只保留解释、一个主 CTA 和确有价值的帮助链接；首条数据出现后再展示过滤、批量与实时控制。若过滤可能导致“假空”，则应显示“清除过滤条件”，而不是创建资源 CTA。

### P2-5 模型能力标签只有 9px，并在移动端被统一截断

**严重度：中**  
**范围：** 模型目录桌面表格与移动卡片。

普通桌面单元格是合格的 14px/54px，但 capability pills 实测只有 9px。移动端每个标签还设置了 `overflow: hidden; text-overflow: ellipsis`：例如 `tools` 的可见宽度 28.9px、实际内容宽度 31px，`temperature` 为 63.7px/69px。结果是几乎每个能力名称都以省略号结束，页面同时堆满大量全大写胶囊，既难读又产生 badge spam。

**建议：** 能力文字至少 12px；桌面优先用普通文字/图标组合而非每项一个 pill；移动端显示 2–3 个关键能力加“另外 N 项”，点击后在详情区完整展示，不能把每个词本身截断。

### P2-6 移动模型目录默认 25 条，分页要滚动 5.2 个屏高才能到达

**严重度：中**  
**范围：** 375×812 的模型目录。

移动端正确把表格改成卡片，但仍沿用桌面每页 25 条。实测页面高度 4216px，是视口高度的 5.19 倍，分页和每页数量控件位于最底部。重复的“复制 ID / 类型 / 能力”结构让扫描成本很高。

**建议：** 移动端默认每页 10 条，或使用“加载更多”；将模型名、类型和最重要能力合并成更紧凑的两行摘要，详情按需展开。分页控件应在长列表顶部提供结果计数，底部再重复一次。

### P2-7 模型目录区块的组外间距不足

**严重度：中**  
**范围：** 模型目录桌面与移动布局。

“本地目录覆盖”折叠条与下一段 `模型目录` H1 几乎贴在一起；移动端标题甚至视觉上压住上一块边界。组外间距小于卡片内部的间距，使用户难以判断标题属于上一个折叠区还是下一个列表区。

**建议：** 折叠区结束后使用明确的 24px 组外间距，标题—说明—搜索之间使用 8/12/16px 组内阶梯；不要靠相邻边框承担分组。

### P2-8 添加渠道弹窗把原始 JSON 当作基础任务的一部分

**严重度：中**  
**范围：** 添加渠道弹窗，375px 最明显。

弹窗首屏依次出现名称、类型、Base URL 后，立即展示大面积“高级设置”JSON 编辑框；它占据主要视觉空间，把最常见的“添加一个渠道”任务变成配置文件编辑。与此同时，“状态”使用带勾圆形 checkbox，而设计源规定布尔值统一使用 34×20px 的 switch，组件语言不一致。

**建议：** 默认只保留名称、预设/类型、Base URL、凭据关联和启用状态；原始 JSON 放进“高级设置”折叠区，并优先用结构化字段承接常用配置。状态控件统一使用设计系统 switch。

## 其他自动检查结果

- 概览 KPI 容器把 `aria-label` 放在无有效 role 的 `div` 上，axe 标为 incomplete；建议给容器增加合适的语义角色或移除无效属性。
- 模型目录的桌面表格本体没有 axe violation；部分被粘性层遮挡的行无法完成自动对比度判定，需要人工抽检。
- 页面在 375px 没有 body 级横向滚动，这一点符合响应式约束。

## 建议修复顺序

1. 先修共享 Button 的 `accent-contrast` 和共享 Modal 的关闭按钮名称；这是全局组件修复，收益最大。
2. 修移动 AppShell 抽屉的关闭态几何与双重头部。
3. 统一 onboarding、标题区、空状态三层操作优先级，并按数据状态隐藏无效控件。
4. 重做模型能力信息密度和移动分页；同时修复目录标题的组外间距。
5. 最后精简添加渠道弹窗，把 JSON 和低频选项移入渐进披露层。

## 验收标准

- 所有主题组合下，14px 主按钮文字对比度不低于 `4.5:1`。
- 所有 icon-only 控件有本地化 accessible name，弹窗 axe 无 critical/serious violation。
- 375/768px 抽屉关闭时不露边框或阴影，打开后只出现一套品牌头和一个关闭按钮。
- 每个资源页最多一个实心主操作；空状态不展示无效批量/过滤/暂停控件。
- 模型能力不低于 12px，不截断单个能力词；移动端分页在约 2–3 个屏高内可达。
- 375、768、1440px 复测浅/深色和中/英文，页面无横向滚动，组外间距明确大于组内间距。

## 本地证据

本轮浏览器截图保存在 `/tmp/pangolin-refactoring-ui-audit/screenshots/`，未加入仓库。关键文件包括：

- `overview-375-dark-zh-full-raw.png`
- `channels-375-light-en-full-raw.png`
- `nav-drawer-375-dark-zh-settled-raw.png`
- `add-channel-dialog-375-dark-zh-raw.png`
- `model-catalog-1440-dark-zh-raw.png`
- `model-catalog-375-dark-zh-closed-full-raw.png`
- `system-375-dark-zh-full-raw.png`

## 2026-09-22 复测（Batch B1）

> 复测对象：重新构建的 release binary（`http://127.0.0.1:18120`，已填充渠道、凭据、模型、探针和一次成功 Responses 请求）。
> 方法：浏览器 axe 4.12.1 与尺寸测量在控制端执行；本批把可复现的对比度与响应式缺陷改成源码级 red/green 测试，并只按测量结果修改颜色与布局。

### 旧结论的关闭

| 旧编号 | 旧结论 | 现状 |
| --- | --- | --- |
| P1-1 | 深色主按钮文字对比度不达标 | 已关闭：filled 按钮、分页激活项与勾选框统一消费 `--accent-contrast`（`web/src/mantine.ts` Button/Pagination/Checkbox 的 `vars`）。 |
| P1-2 | 弹窗关闭按钮无可访问名称 | 已关闭：共享 Modal 与资源弹窗都传 `closeButtonProps={{ 'aria-label': t('close') }}`（`web/src/components.tsx:33`、`web/src/pages/shared.tsx:202`），axe 不再报 `button-name`。 |
| P2-1 | 关闭态移动抽屉露出 6px 边框 | 已关闭：`.sidebar` 关闭态 `visibility: hidden` + `translateX(-100%)`，退出后不再留边（`web/src/styles.css:451`）。 |
| P2-2 | 移动抽屉出现两套品牌头和两个关闭按钮 | 已关闭：抽屉内品牌头 `visibleFrom="md"`，移动端只保留顶栏的一套品牌与开关（`web/src/Shell.tsx:95`）。 |
| P2-5 | 能力标签 9px 且被截断 | 已关闭：能力值改为 12.5px 参考文本、以 `·` 分隔且不截断（`web/src/styles.css:107`）。 |
| P2-6 | 移动模型目录默认 25 条 | 已关闭：`CATALOG_MOBILE_PAGE_SIZE = 10`（`web/src/pages/ModelsPage.tsx:132`）。 |
| P2-7 | 目录区块组外间距不足 | 已关闭：`CATALOG_GROUP_GAP = 24`，组内保持 ≤16px（`web/src/pages/ModelsPage.tsx:130`）。 |
| P2-8 | “高级设置”原始 JSON 占据首屏、状态用勾选圆框 | 已关闭：JSON 字段改为 `Collapse` 渐进披露（`web/src/pages/shared.tsx:306`），状态字段改为设计系统 switch（`web/src/pages/shared.tsx:255`）。 |
| 其他自动检查 | 375px “英文数据表被裁切” | **未复现**：375×812 英文、有数据的探针页确实切换为卡片，`scrollWidth == 375`，body/document 无横向滚动。jsdom 无法测量溢出，因此不新增该断言，也不再据此改动表格。 |

仍未关闭：P2-3（首次引导与页面主操作竞争）已通过“仅概览页展示完整横幅、其他页面降级为可关闭提示”部分缓解，是否进一步收敛留待后续批次；P2-4（空状态仍展示无效过滤器/批量/暂停控件）本轮未复测。

### 本批新测得的两处对比度缺陷（已修复）

| 位置 | 复测值 | 根因 |
| --- | --- | --- |
| 模型与渠道页 `ENABLED` 徽章 | 4.32:1（`#087f5b` on `#c3fae8`，11px bold） | 只有 `pangolin` 家族映射到调色板，`variant="light"` 的 teal/green/yellow/red 仍取 Mantine 自带 ramp（teal-9 / teal-1） |
| 失败探针弹窗错误文案 | 2.86:1（`#fa5252` on `#f2efe9`，12.5px normal） | `<Text c="red">` 解析到 Mantine red-6，未走语义 token |

同一根因还影响健康列与探针表：`HealthPill` 的告警分支源码计算为 2.69:1（`#e67700` on `#fff3bf`）、探针表成功徽章为 3.81:1（`#2b8a3e` on `#d3f9d8`）；两者在本次有数据状态下没有出现在 axe 报告里，但同样低于 AA，随本次修复一并解决。有数据的 `FAILED` 徽章（`#c92a2a` on `#ffe3e3`，4.51:1）与失败探针行的 `HealthPill` 本身没有 axe violation，符合控制端结论。

修复方式：

- `web/src/mantine.ts` 的 resolver 为 teal/green/yellow/red 增加 `light` / `light-hover` / `light-color` 映射，分别指向 `--success(-soft)`、`--warning(-soft)`、`--danger(-soft)`；调色板 guard 已保证这些配对在两种模式下 ≥4.5:1，因此是共享 token 级修复而非逐个徽章改色。
- 失败文案改用语义样式 `.error-text { color: var(--danger) }`（`web/src/styles.css`），`web/src/pages/ChannelsPage.tsx:247` 由 `c="red"` 改为该 class，错误语义不变。
- 审查追加（fix round 1）：`Button variant="outline" color="red"`（`InlineQueryError` 的重试按钮，位于 `--danger-soft` 告警底上）此前仍取 Mantine red-6，普通文字约 3.0:1。resolver 现额外把 `-outline`、`-outline-hover`、`-text` 映射到 danger 家族：`-outline` 取 `--danger`，hover 填充取「danger-soft 55% + surface」，使其在两种模式下都远离文字色（悬停后 5.35:1 / 5.75:1；若沿用 accent 的「soft + 12% 主色」写法会掉到 4.15:1 / 4.30:1），`-text` 为当前无消费者的别名。`filled` 未改动，破坏性确认按钮的观感不变。

### 375px 首次引导横幅（已修复）

- 复现：横幅行是 `nowrap`，操作按钮保持自身宽度，375px 下说明文字被挤成约 150px 一列、几乎逐词换行（证据：`/tmp/pangolin-parity-ui/screenshots/probes-en-light-375.png`）。
- 修复：`web/src/styles.css` 新增 `@media (max-width: 40em)`，横幅行允许换行、说明占满整行，唯一的操作与本地化命名的关闭控件在其下组成一个紧凑块；桌面两列布局不变。
- 测试：`consoleFixes.test.tsx` 断言该响应式契约，`onboarding.test.tsx` 断言行内只有一个操作和一个可访问关闭控件。

### 本批新增/更新的测试与证据

- `web/src/consoleFixes.test.tsx`：`status pills paint palette ink instead of Mantine's default ramp`、`failure copy uses the semantic danger ink`、`the onboarding banner stacks below the mobile breakpoint`、`the inline query error retry reads at AA`（fix round 1；bronze/slate/jade × light/dark，比较原始比值，格式化只用于失败信息）。
- `web/src/onboarding.test.tsx`：`keeps one action and one named dismiss control in the banner row`。
- RED：`.superpowers/sdd/parity-remaining-handover/evidence/task-b1-red.txt`（9 项失败）、`task-b1-fix1-red.txt`（6 项失败）；GREEN：`task-b1-green-focused.txt`（2 文件 / 25 测试）、`task-b1-green-full.txt`（58 文件 / 1348 测试）、`task-b1-fix1-green-focused.txt`（2 文件 / 33 测试）、`task-b1-fix1-green-full.txt`（58 文件 / 1356 测试）、`task-b1-fix2-green-focused.txt`（1 文件 / 30 测试）、`task-b1-fix2-green-full.txt`（58 文件 / 1356 测试）。
- fix round 2 为机械收尾：column-header 的两处对比度断言（原 `web/src/consoleFixes.test.tsx:102`、`:108`）同样改为直接比较原始比值（实测 5.41:1 / 6.29:1），断言结果不变，故无可演示的 RED；该文件内所有 `toFixed` 现仅用于失败信息。
- 完整实现报告（含 fix round 1、2）：`.superpowers/sdd/parity-remaining-handover/task-b1-ui-measurement-report.md`。
- 控制端重建 release binary 后复测：Models（light/en/1440）、Channels（light/en/1440，含 HEALTHY/ENABLED）、失败探针弹窗（light/en/1440）和 Channels（dark/en/1440）均为 axe 4.12.1 WCAG 2 AA 零 violation；375×812 稳定态 body/document/viewport 宽度均为 375px，引导文案与操作/关闭控件已分两行清晰呈现。截图保存在 `/tmp/pangolin-parity-ui/screenshots/*-b1-fixed*.png`。
