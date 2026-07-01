# 扫描键功能优化改造方案

> **实现状态**：✅ 已实现  
> **关键代码**：`src/views/KeyMain.vue`（`scanCancelled`、`scanKeyAuto`、`scanPaused` 等）  
> **实际实现**：`KeyMain.vue` 中 `scanKeyAuto` 递归 SCAN + `scanPaused` 暂停/继续 + `scanCancelled` 取消；边扫边显示；§五 SCAN 动态 COUNT **未实现**。

## 实际实现（RedisME）

| 机制      | 实现                                                       |
| --------- | ---------------------------------------------------------- |
| 扫描驱动  | 前端 `scanKeyAuto` 循环调用后端 `scanKey`，非 Node Stream  |
| 暂停/继续 | `scanPaused` + `canShowScanControl`                        |
| 取消      | `scanCancelled`，Abort 当前批次                            |
| 批量大小  | 固定 `scanBatchSize` / settings.keyScanCount，无动态 COUNT |
| UI        | 底部扫描控制：暂停/继续/停止                               |

> 参考项目：AnotherRedisDesktopManager、RedisInsight、tiny-rdm  
> 目标：速度快、不卡死、边扫边显示

---

## 一、三款 RDM 扫描键策略分析

### 1. AnotherRedisDesktopManager (ARDM)

**技术栈**：Node.js + Vue + Electron

**核心机制**：Node.js Stream API (`scanBufferStream`)

```javascript
// 每个节点创建一个 stream
const stream = node.scanBufferStream({ match, count })
this.scanStreams.push(stream)

// 实时数据回调
stream.on('data', keys => {
  this.keyList = this.keyList.concat(keys)
  this.onePageKeysCount += keys.length

  // 达到一页大小时暂停
  if (this.onePageKeysCount >= keysPageSize && !loadAll) {
    stream.pause()
  }
})

// 加载更多时恢复
stream.resume()
```

**特点**：

- **流式实时显示**：`data` 事件实时返回匹配的 key，用户立即看到结果
- **分页控制**：`stream.pause()` / `stream.resume()` 控制扫描节奏
- **状态反馈**：搜索框显示 loading / cancel 图标，可取消扫描
- **集群支持**：每个 master 节点创建独立 stream

**优点**：

- 用户体验好，边扫边显示，无等待感
- 内存友好，流式处理，不需要一次性加载所有结果
- 支持取消扫描，防止误操作后长时间等待

**缺点**：

- 依赖 Node.js Stream API，Tauri 环境无法直接使用
- 需要额外处理 stream 错误和结束事件

---

### 2. RedisInsight

**技术栈**：React + Node.js (后端 API)

**核心机制**：HTTP API 请求 + cursor 分页

```typescript
// 前端通过 HTTP 请求后端 SCAN
const { data } = await apiService.post(`/databases/${id}/keys`, {
  cursor,
  count,
  type,
  match,
  scanThreshold, // 扫描阈值配置
})

// 加载更多
dispatch(loadMoreKeysSuccess({ keys: oldKeys.concat(newKeys), cursor: nextCursor }))
```

**特点**：

- **前后端分离**：前端纯 UI，后端执行 SCAN
- **请求取消**：使用 `CancelToken` 支持取消扫描
- **配置阈值**：`scanThreshold` 控制单次扫描量
- **分页加载**：每次请求带 cursor，分批返回

**优点**：

- 架构清晰，前后端职责分离
- 支持取消，用户体验好
- 可配置扫描阈值，适应不同规模的数据库

**缺点**：

- HTTP 往返开销较大
- 实时性不如 Stream 模式

---

### 3. tiny-rdm

**技术栈**：Go (Wails) + Vue

**核心机制**：后端 Go 执行 SCAN，前端轮询

```go
// 后端 scanKeys 函数
func (b *browserService) scanKeys(ctx context.Context, client redis.UniversalClient,
  match, keyType string, cursor uint64, count int64) ([]any, uint64, error) {

  scanSize := int64(Preferences().GetScanSize()) // 默认 3000
  scanCount := int64(0)

  for {
    if filterType {
      loadedKey, cursor, err = cli.ScanType(ctx, cursor, match, scanSize, keyType).Result()
    } else {
      loadedKey, cursor, err = cli.Scan(ctx, cursor, match, scanSize).Result()
    }
    // ...
    scanCount += int64(len(ks))
    if (count > 0 && scanCount > count) || cursor == 0 {
      break
    }
  }
}
```

**三种加载模式**：

- `LoadNextKeys`：加载下一批（带 cursor，分页）
- `LoadNextAllKeys`：加载剩余所有（cursor=0, count=0 无限制）
- `LoadAllKeys`：从头加载所有

**特点**：

- **后端控制**：扫描逻辑完全在后端
- **配置扫描大小**：`scanSize` 可配置
- **集群支持**：`ForEachMaster` 遍历所有 master

**优点**：

- 前端简单，纯展示
- 后端可灵活控制扫描策略

**缺点**：

- `LoadNextAllKeys` / `LoadAllKeys` 后端可能长时间阻塞
- 无取消功能
- 前端无法实时看到中间结果

---

## 二、三款 RDM 综合对比

| 维度           | ARDM                | RedisInsight      | tiny-rdm       | redis-me(当前)      |
| -------------- | ------------------- | ----------------- | -------------- | ------------------- |
| **扫描方式**   | Stream API          | HTTP API + cursor | Go 后端 SCAN   | Tauri 命令 + cursor |
| **实时显示**   | ✅ 流式实时         | ❌ 分批返回       | ❌ 分批返回    | ✅ 分批返回         |
| **分页控制**   | stream.pause/resume | cursor 参数       | cursor + count | async/await 递归    |
| **加载全部**   | 大 COUNT            | scanThreshold     | count=0        | 前端递归轮询        |
| **取消扫描**   | ✅ stream.destroy   | ✅ CancelToken    | ❌             | ✅ scanCancelled    |
| **前端复杂度** | 高                  | 低                | 低             | 中                  |
| **后端复杂度** | 低                  | 中                | 高             | 中                  |

---

## 三、改造方案设计

### 目标

1. **速度快**：减少单次请求时间，利用分批加载避免长时间等待
2. **不卡死**：后端限制单次 SCAN 迭代次数，前端异步轮询
3. **边扫边显示**：用户能实时看到扫描结果，无需等待全部完成

### 核心策略

结合三款 RDM 的优点，采用 **"后端限流 + 前端轮询 + 实时显示"** 策略：

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   用户操作   │────▶│   前端递归   │────▶│  后端 SCAN   │
│  搜索/加载  │     │ async/await │     │  单次执行    │
└─────────────┘     └─────────────┘     └─────────────┘
       │                   │                   │
       │              条件判断                返回结果
       │                   │                   │
       ▼                   ▼                   ▼
  ┌─────────────────────────────────────────────────┐
  │            实时显示扫描结果                      │
  │   - 搜索时：有结果立即停止，用户立即看到          │
  │   - 加载全部：递归循环直到扫完                    │
  │   - 用户可取消：设置标志位停止后续递归            │
  └─────────────────────────────────────────────────┘
```

### 核心策略

结合三款 RDM 的优点，采用 **"异步递归 + 可取消 + 实时显示"** 策略：

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   用户操作   │────▶│   前端递归   │────▶│  后端 SCAN   │
│  搜索/加载  │     │ async/await │     │  单次执行    │
└─────────────┘     └─────────────┘     └─────────────┘
       │                   │                   │
       │              条件判断                返回结果
       │                   │                   │
       ▼                   ▼                   ▼
  ┌─────────────────────────────────────────────────┐
  │            实时显示扫描结果                      │
  │   - 搜索时：有结果立即停止，用户立即看到          │
  │   - 加载全部：递归循环直到扫完                    │
  │   - 用户可取消：设置标志位停止后续递归            │
  └─────────────────────────────────────────────────┘
```

### 具体改造点

#### 1. 异步递归扫描（已实现 ✅）

```typescript
// loadAll 模式：递归加载直到扫描完成
async function scanKeyAll(): Promise<void> {
  if (!cursor.value || cursor.value.finished) return
  if (scanCancelled.value) return // 用户取消则停止

  await scanKeyCore(true)
  await scanKeyAll() // 继续递归
}
```

- **async/await 递归**：每次 `await` 释放调用栈，不会栈溢出
- **无 setTimeout 轮询**：直接递归调用，结果即时返回

---

## 四、实现效果

### 搜索场景

```
用户输入搜索关键词
       │
       ▼
  ┌──────────┐
  │ 发起搜索  │
  └────┬─────┘
       │
       ▼
  ┌──────────┐
  │ 后端扫描  │────▶│ 返回结果
  └────┬─────┘     └────┬─────┘
       │                │
       │           有匹配结果
       │                │
       │                ▼
       │           ┌──────────┐
       │           │ 停止加载  │
       │           └────┬─────┘
       │                │
       │           无匹配结果
       │                │
       │                ▼
       │           ┌──────────┐
       │           │ 前端递归  │
       │           │ 继续扫描  │
       │           └────┬─────┘
       │                │
       ▼                ▼
  ┌─────────────────────────┐
  │      显示结果，停止扫描   │
  └─────────────────────────┘
```

### 加载全部场景

```
用户点击"加载全部"
       │
       ▼
  ┌──────────┐
  │ 递归扫描  │
  └────┬─────┘
       │
       ▼
  ┌─────────────────────────────────┐
  │  批次1  │  批次2  │  批次3  │ ... │
  │  递归   │  递归   │  递归   │     │
  └─────────────────────────────────┘
       │
       │ 结果实时追加到列表
       │
       ▼
  cursor.finished（扫描完成）
       │
       ▼
  ┌─────────────────┐
  │  释放 loading   │
  └─────────────────┘
```

---

## 五、总结

本次改造参考了三款主流 RDM 的扫描键策略，综合各自优点：

- **ARDM**：流式实时显示、状态反馈、取消扫描
- **RedisInsight**：前后端分离、请求取消、配置化
- **tiny-rdm**：后端控制扫描逻辑、配置扫描大小

**当前已实现**：

- ✅ 异步递归扫描（async/await，不会栈溢出）
- ✅ 搜索自动加载（有结果即停止）
- ✅ 可取消扫描（scanCancelled 标志位）
- ✅ loading 统一状态管理
- ✅ 空状态优化（扫描中显示"扫描中..."）

**待实现**：

- 💡 SCAN 迭代次数动态调整（可选）

---

## 六、最终改造方案（v2.0 - 已实现 ✅）

### 核心思路

**异步递归**：前端使用 async/await 递归执行扫描，每次 `await` 释放调用栈，不会栈溢出。
**可取消**：用户点击取消后，设置 `scanCancelled = true`，当前轮次完成后停止后续递归。
**自动加载**：搜索时达到阈值（默认 keyScanCount）自动停止，避免过度扫描。
**即时显示**：每次 SCAN 请求返回后立即更新列表，用户能实时看到结果。

### 前端改造

#### 新增取消扫描状态

```typescript
const scanCancelled = ref(false) // 扫描是否被取消
```

#### 搜索时重置取消标志

```typescript
async function scanKey(useCursor = false, loadAll = false): Promise<void> {
  if (loading.value || !share.conn) return

  loading.value = true
  scanCancelled.value = false // 每次扫描都重置取消标志
  try {
    if (!useCursor) {
      cursor.value = null
    }

    const firstScanKeys = await scanKeyCore(useCursor)

    // loadAll=false 时自动继续加载（达到阈值停止）
    if (!loadAll) {
      await scanKeyAuto(firstScanKeys)
    } else {
      await scanKeyAll()
    }
  } finally {
    loading.value = false
  }
}
```

#### 循环前检查是否已取消

```typescript
// 自动加载：递归执行直到满足停止条件（async/await 不会栈溢出）
async function scanKeyAuto(fetchedCount: number = 0): Promise<void> {
  if (!cursor.value || cursor.value.finished) return
  if (scanCancelled.value) return
  if (fetchedCount >= SCAN_FETCH_COUNT.value) return

  const newKeys = await scanKeyCore(true)
  await scanKeyAuto(fetchedCount + newKeys)
}

// 加载全部：递归执行直到扫描完成（async/await 不会栈溢出）
async function scanKeyAll(): Promise<void> {
  if (!cursor.value || cursor.value.finished) return
  if (scanCancelled.value) return

  await scanKeyCore(true)
  await scanKeyAll() // 继续递归
}
```

#### 取消扫描方法

```typescript
function cancelScanning() {
  scanCancelled.value = true
}
```

### UI 交互设计

#### 按钮状态流转

```
初始状态：
┌─────────────────────────────────┐
│  [搜索框]  [🔍刷新键] [➕新增键] │
└─────────────────────────────────┘

扫描中：
┌─────────────────────────────────┐
│  [搜索框 ⏳] [⏳扫描中...] [❌停止扫描] │
└─────────────────────────────────┘

点击取消后：
┌─────────────────────────────────┐
│  [搜索框]  [🔍刷新键] [➕新增键] │
│         已加载 150/10000 条      │
└─────────────────────────────────┘
```

#### 实现细节

- **点击"刷新键"或按 Enter**：开始扫描，搜索框显示 loading 图标，新增键按钮变为"停止扫描"
- **点击"停止扫描"**：设置 `scanCancelled = true`，当前轮次完成后停止
- **搜索框 readonly**：扫描时禁止输入，保持视觉一致性
- **结果保留**：取消后保留已加载的结果，不清空列表
- **空状态优化**：扫描中显示"扫描中..."，扫描完成后无结果显示"没有数据"

### 边界情况处理

| 场景     | 处理逻辑                                         |
| -------- | ------------------------------------------------ |
| 正常完成 | `cursor.finished = true`，自动释放 loading       |
| 用户取消 | `scanCancelled = true`，当前轮次结束停止         |
| 新搜索   | 自动重置 `scanCancelled = false`                 |
| 快速取消 | 如果请求已发出，等待当前请求完成后停止           |
| 首次扫描 | 空列表且 loading 时显示"扫描中..."而非"没有数据" |

### 关键优化点

#### 1. scanKeyCore 返回新扫描数量

```typescript
async function scanKeyCore(useCursor = false): Promise<number> {
  const params = {
    match: match.value,
    type: keyType.value === 'ALL' ? '' : keyType.value.toLowerCase(),
    cursor: cursor.value,
  }

  const data = await meCommands.scan(share.conn!.id, params)
  cursor.value = data.cursor

  const newKeyList = useCursor ? [...keyList.value, ...data.keyList] : data.keyList
  keyList.value = sortBy(newKeyList, ['key'])

  return data.keyList.length // 直接返回本次扫描的 key 数量
}
```

**优点**：

- 简化调用方逻辑，无需通过 `beforeLength` 计算
- 代码更清晰，意图明确

#### 2. 删除 autoContinue 参数

```typescript
// 旧版：三个参数
async function scanKey(useCursor = false, loadAll = false, autoContinue = false)

// 新版：两个参数，语义更清晰
async function scanKey(useCursor = false, loadAll = false)
```

**逻辑**：

- `loadAll = false`：自动继续加载，达到阈值（SCAN_FETCH_COUNT）停止
- `loadAll = true`：加载全部，直到扫描完成

#### 3. async/await 递归不会栈溢出

```typescript
// 注释说明：async/await 不会栈溢出
async function scanKeyAuto(fetchedCount: number = 0): Promise<void> {
  if (!cursor.value || cursor.value.finished) return
  if (scanCancelled.value) return
  if (fetchedCount >= SCAN_FETCH_COUNT.value) return

  const newKeys = await scanKeyCore(true)
  await scanKeyAuto(fetchedCount + newKeys)
}
```

**原理**：每次 `await` 都会释放调用栈，不会导致栈溢出

#### 4. i18n 精确化

```typescript
// zh-cn.ts
scanning: '扫描中...',
stopScan: '停止扫描',

// en.ts
scanning: 'Scanning...',
stopScan: 'Stop Scan',
```

### 改造优先级（更新后）

| 优先级 | 改造点               | 状态      | 说明                   |
| ------ | -------------------- | --------- | ---------------------- |
| P0     | 异步递归扫描         | ✅ 已完成 | async/await 不栈溢出   |
| P0     | 可取消扫描           | ✅ 已完成 | scanCancelled 标志位   |
| P0     | 搜索自动加载         | ✅ 已完成 | 有结果立即停止         |
| P0     | 空状态优化           | ✅ 已完成 | 扫描中显示"扫描中..."  |
| P0     | scanKey 简化         | ✅ 已完成 | 删除 autoContinue 参数 |
| P0     | scanKeyCore 返回值   | ✅ 已完成 | 返回新扫描数量         |
| P1     | loading 统一状态管理 | ✅ 已完成 | 避免 true→false→true   |
| P1     | 搜索自动加载         | ✅ 已完成 | 有结果即停止           |

---

_方案文档更新于 2026-06-18_
