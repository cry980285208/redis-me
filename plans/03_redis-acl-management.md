# Redis ACL 管理 UI - 实现方案

> **实现状态**：✅ 已实现  
> **关键代码**：`src/views/tab/RedisACL.vue`、`src/views/ext/UserAdd.vue`、`AclLog.vue`、`AclDryrun.vue`、`src/utils/acl.ts`  
> **实际实现**：用户 CRUD、ACL 日志、DRYRUN、重置密码均在 `UserAdd.vue` 及相关 Tab 完成。

## 一、需求分析

Redis 6.0+ 引入了 ACL（Access Control List）系统，可以通过命令控制用户权限。当前项目中已有 ACL 命令的定义和配置提示，但缺少可视化管理界面。

**目标**：在现有标签页基础上新增 ACL 管理标签页，提供简洁的用户管理和权限分配功能。

## 二、技术约束

- **Redis 版本要求**：Redis 6.0+（Valkey 同样支持）
- **权限要求**：当前连接用户需具有 `admin` 权限或为 `default` 用户
- **集群兼容**：集群模式下 ACL 配置会在各节点同步
- **只读模式**：连接为只读时，禁用编辑功能

## 三、UI 设计方案

### 整体风格

参照现有标签页（如 `RedisConfig.vue`、`RedisMemory.vue`）的简洁风格，保持：

- **主文件**：`src/views/tab/RedisACL.vue`
- **用户编辑对话框**：`src/views/ext/UserAdd.vue`（非独立 `UserAdd.vue`）

### 主界面布局

```
┌─────────────────────────────────────────────────────┐
│ [新建用户]  [刷新]                    [搜索用户...] │
├─────────────────────────────────────────────────────┤
│  用户列表表格                                        │
│  ┌──────┬──────┬──────────┬──────────┬──────────┐  │
│  │ 用户名│ 状态 │ 命令权限 │ 键模式   │ 操作     │  │
│  ├──────┼──────┼──────────┼──────────┼──────────┤  │
│  │default│ ●启用│ +@all    │ ~*       │ [编辑]   │  │
│  │user1  │ ○禁用│ +@read   │ ~cache:* │ [编辑]   │  │
│  │user2  │ ●启用│ +get,-set│ ~user:*  │ [编辑]   │  │
│  └──────┴──────┴──────────┴──────────┴──────────┘  │
└─────────────────────────────────────────────────────┘
```

**主界面功能**：

1. 顶部工具栏：新建用户、刷新、搜索框
2. 表格展示用户列表：
   - 用户名
   - 状态（启用/禁用标签）
   - 命令权限摘要（如 `+@all`、`+@read,-set`）
   - 键模式摘要（如 `~*`、`~cache:*`）
   - 操作列：编辑、删除按钮
3. 受 `share.readonly` 控制显示/隐藏编辑按钮

### 编辑对话框布局

```
┌──────────────────────────────────────┐
│  编辑用户: user1                      │
│  ┌────────────────────────────────┐  │
│  │ 用户名: [user1] (新建可编辑)    │  │
│  │ 状态: ● 启用 ○ 禁用            │  │
│  │ 密码: [••••••] [生成随机密码]   │  │
│  │                                │  │
│  │ 命令权限:                       │  │
│  │ [+@all] [+@read] [+@write]     │  │
│  │ [-@dangerous]                  │  │
│  │ [输入自定义权限...]             │  │
│  │ 当前: +@all, -set, +get        │  │
│  │                                │  │
│  │ 键模式:                         │  │
│  │ [~cache:* ×] [~user:* ×]       │  │
│  │ [添加键模式...]                 │  │
│  │                                │  │
│  │ 频道权限:                       │  │
│  │ [&channel:* ×]                 │  │
│  │ [添加频道模式...]               │  │
│  └────────────────────────────────┘  │
│         [取消]  [保存] [删除用户]     │
└──────────────────────────────────────┘
```

**编辑对话框功能**：

1. 基本信息：用户名、启用/禁用开关、密码设置
2. 命令权限：
   - 快捷类别按钮：`+@all`、`+@read`、`+@write`、`+@admin`、`-@dangerous` 等
   - 自定义输入：文本框输入 `+get`、`-set` 等
   - 当前权限展示：Tag 列表，可删除
3. 键模式：Tag 列表 + 添加/删除（`~pattern` 语法）
4. 频道权限：Tag 列表 + 添加/删除（`&pattern` 语法）
5. 快捷操作：生成随机密码、删除用户

## 四、文件结构

```
src/
├── views/
│   └── tab/
│       └── RedisACL.vue              # ACL 主标签页
├── components/
│   └── acl/
│       └── UserAdd.vue              # 用户编辑表单对话框（可选）
└── locales/
    └── lang/
        ├── zh-CN.json                # 中文翻译
        └── en.json                   # 英文翻译

src-tauri/
└── src/
    └── api.rs                        # 新增 ACL 相关 Tauri 命令
```

## 五、后端 API 设计

在 `src-tauri/src/api.rs` 中新增以下 Tauri 命令：

```rust
/// 获取所有 ACL 用户列表
#[tauri::command]
async fn acl_users(conn_id: String) -> Result<Vec<String>, AppError> {
    // 执行 ACL USERS 命令
}

/// 获取指定用户的详细信息
#[tauri::command]
async fn acl_getuser(conn_id: String, username: String) -> Result<ACLUserDetail, AppError> {
    // 执行 ACL GETUSER 命令
}

/// 创建或修改用户
#[tauri::command]
async fn acl_setuser(conn_id: String, username: String, rules: Vec<String>) -> Result<(), AppError> {
    // 执行 ACL SETUSER username rule1 rule2 ...
}

/// 删除用户
#[tauri::command]
async fn acl_deluser(conn_id: String, usernames: Vec<String>) -> Result<i64, AppError> {
    // 执行 ACL DELUSER user1 user2 ...
}

/// 生成随机密码
#[tauri::command]
async fn acl_genpass(conn_id: String, bits: Option<u32>) -> Result<String, AppError> {
    // 执行 ACL GENPASS [bits]
}

/// 获取 ACL 日志
#[tauri::command]
async fn acl_log(conn_id: String, count: Option<usize>) -> Result<Vec<ACLEntry>, AppError> {
    // 执行 ACL LOG [count]
}

/// 清空 ACL 日志
#[tauri::command]
async fn acl_log_reset(conn_id: String) -> Result<(), AppError> {
    // 执行 ACL LOG RESET
}

/// 获取 ACL 规则列表（文件格式）
#[tauri::command]
async fn acl_list(conn_id: String) -> Result<Vec<String>, AppError> {
    // 执行 ACL LIST 命令
}
```

### 数据模型

```rust
// src-tauri/src/utils/model.rs 中新增

/// ACL 用户详情
#[derive(Debug, Serialize, Deserialize)]
pub struct ACLUserDetail {
    pub username: String,
    pub flags: Vec<String>,           // on/off, nopass 等
    pub passwords: Vec<String>,       // 密码哈希列表
    pub commands: Vec<String>,        // +get, -set, +@admin 等
    pub keys: Vec<String>,            // ~pattern
    pub channels: Vec<String>,        // &pattern
    pub selectors: Vec<String>,       // 选择器权限
}

/// ACL 设置规则
#[derive(Debug, Serialize, Deserialize)]
pub struct ACLSetRules {
    pub username: String,
    pub enabled: bool,                // on/off
    pub passwords: Vec<String>,       // 密码列表
    pub commands: Vec<String>,        // 命令权限规则
    pub keys: Vec<String>,            // 键模式
    pub channels: Vec<String>,        // 频道模式
}

/// ACL 日志条目
#[derive(Debug, Serialize, Deserialize)]
pub struct ACLEntry {
    pub count: i64,
    pub reason: String,               // auth, command, key, channel
    pub context: String,
    pub object: String,
    pub username: String,
    pub age_seconds: f64,
    pub client_info: String,
}
```

## 六、前端 TypeScript 类型

```typescript
// src/views/tab/RedisACL.vue 中定义

interface ACLUser {
  username: string
  flags: string[] // on, off, nopass 等
  passwords: string[]
  commands: string[] // +get, -set, +@admin 等
  keys: string[] // ~pattern
  channels: string[] // &pattern
}

interface UserFormData {
  username: string
  enabled: boolean
  passwords: string[]
  commandRules: string[] // 权限规则数组
  keyPatterns: string[]
  channelPatterns: string[]
}
```

## 七、核心实现要点

### 7.1 用户列表展示

```javascript
// RedisACL.vue 核心逻辑
import { meCommands, meConfirm, meOk } from '@/utils/util'

// 获取用户列表
async function fetchUsers() {
  const usernames = await meCommands.aclUsers(share.conn!.id)
  // 并行获取每个用户的详情
  const users = await Promise.all(
    usernames.map(username => meCommands.aclGetuser(share.conn!.id, username)),
  )
  dataList.value = users
}

// 命令权限摘要展示
function formatCommands(commands) {
  // 将 ['+get', '-set', '+@read'] 格式化显示
  return commands.slice(0, 5).join(', ') + (commands.length > 5 ? '...' : '')
}

// 删除用户
async function deleteUser(username) {
  await meConfirm(`确定删除用户 ${username} 吗？`, async () => {
    await meCommands.aclDeluser(share.conn!.id, [username])
    meOk('用户已删除')
    await fetchUsers()
  })
}
```

### 7.2 权限规则构建

```javascript
// UserAdd.vue 核心逻辑
import { meCommands, meOk } from '@/utils/util'

// 构建 ACL SETUSER 命令规则
function buildRules() {
  const rules = []

  // 状态
  rules.push(form.enabled ? 'on' : 'off')

  // 密码
  form.passwords.forEach(pwd => rules.push(`>${pwd}`))

  // 命令权限
  form.commandRules.forEach(rule => rules.push(rule))

  // 键模式
  form.keyPatterns.forEach(pattern => rules.push(`~${pattern}`))

  // 频道模式
  form.channelPatterns.forEach(pattern => rules.push(`&${pattern}`))

  return rules
}

// 保存用户
async function saveUser() {
  const rules = buildRules()
  await meCommands.aclSetuser(share.conn!.id, form.username, rules)
  meOk('用户保存成功')
  emit('success')
}
```

### 7.3 快捷类别按钮

```vue
<!-- 命令权限快捷类别 -->
<div class="command-categories">
  <el-button 
    v-for="cat in aclCategories" 
    :key="cat.rule"
    :type="isSelected(cat.rule) ? 'primary' : 'default'"
    size="small"
    @click="toggleRule(cat.rule)">
    {{ cat.label }}
  </el-button>
</div>

<script>
const aclCategories = [
  { label: '+@all', rule: '+@all' },
  { label: '+@read', rule: '+@read' },
  { label: '+@write', rule: '+@write' },
  { label: '+@admin', rule: '+@admin' },
  { label: '+@fast', rule: '+@fast' },
  { label: '-@dangerous', rule: '-@dangerous' },
  { label: '-@write', rule: '-@write' },
]

function toggleRule(rule) {
  const idx = form.commandRules.indexOf(rule)
  if (idx > -1) {
    form.commandRules.splice(idx, 1) // 移除
  } else {
    form.commandRules.push(rule) // 添加
  }
}
</script>
```

## 八、国际化翻译

### 中文 (zh-CN.json)

```json
{
  "redisACL": {
    "title": "ACL 管理",
    "newUser": "新建用户",
    "refresh": "刷新",
    "searchUser": "搜索用户...",
    "username": "用户名",
    "status": "状态",
    "enabled": "启用",
    "disabled": "禁用",
    "commands": "命令权限",
    "keys": "键模式",
    "channels": "频道权限",
    "password": "密码",
    "generatePassword": "生成随机密码",
    "editUser": "编辑用户",
    "newUserTitle": "新建用户",
    "deleteConfirm": "确定删除用户 {username} 吗？",
    "deleteOk": "用户已删除",
    "saveOk": "用户保存成功",
    "usernameRequired": "请输入用户名",
    "addKeyPattern": "添加键模式",
    "addChannelPattern": "添加频道模式",
    "customRule": "自定义规则",
    "customRulePlaceholder": "如: +get, -set, +@read",
    "addRule": "添加规则",
    "noUsers": "暂无用户",
    "versionRequired": "ACL 管理需要 Redis 6.0+ 版本",
    "hint": "ACL（访问控制列表）用于管理 Redis 用户的权限，包括命令访问、键访问和频道订阅权限"
  }
}
```

### 英文 (en.json)

```json
{
  "redisACL": {
    "title": "ACL Management",
    "newUser": "New User",
    "refresh": "Refresh",
    "searchUser": "Search users...",
    "username": "用户名",
    "status": "Status",
    "enabled": "Enabled",
    "disabled": "Disabled",
    "commands": "Commands",
    "keys": "Key Patterns",
    "channels": "Channels",
    "password": "Password",
    "generatePassword": "Generate Password",
    "editUser": "Edit User",
    "newUserTitle": "New User",
    "deleteConfirm": "Are you sure to delete user {username}?",
    "deleteOk": "User deleted",
    "saveOk": "User saved successfully",
    "usernameRequired": "Please enter username",
    "addKeyPattern": "Add Key Pattern",
    "addChannelPattern": "Add Channel Pattern",
    "customRule": "Custom Rule",
    "customRulePlaceholder": "e.g.: +get, -set, +@read",
    "addRule": "Add Rule",
    "noUsers": "No users",
    "versionRequired": "ACL management requires Redis 6.0+",
    "hint": "ACL (Access Control List) is used to manage Redis user permissions, including command access, key access, and channel subscription permissions"
  }
}
```

## 九、实现步骤

### 阶段一：后端 API（预计 2-3 小时）

1. 在 `src-tauri/src/utils/model.rs` 中添加数据模型
2. 在 `src-tauri/src/api.rs` 中实现 Tauri 命令：
   - `acl_users`
   - `acl_getuser`
   - `acl_setuser`
   - `acl_deluser`
   - `acl_genpass`
3. 在 `src-tauri/src/lib.rs` 中注册命令

### 阶段二：前端主界面（预计 3-4 小时）

1. 创建 `src/views/tab/RedisACL.vue`
2. 实现用户列表表格展示
3. 实现搜索、刷新功能
4. 实现删除用户功能
5. 适配只读模式

### 阶段三：前端表单组件（预计 3-4 小时）

1. 创建 `src/components/acl/UserAdd.vue`
2. 实现用户基本信息编辑
3. 实现命令权限快捷类别 + 自定义输入
4. 实现键模式、频道模式 Tag 输入
5. 实现密码生成器集成

### 阶段四：集成与优化（预计 2-3 小时）

1. 在 `RedisInfo.vue` 信息弹窗中接入 ACL Tab（非 TabMain 新标签页）
2. 添加版本检测（Redis 6.0+）
3. 添加国际化翻译
4. 测试功能完整性
5. 优化 UI 细节

### 阶段五：测试（预计 1-2 小时）

1. 功能测试：新建、编辑、删除用户
2. 权限测试：验证 ACL 规则生效
3. 边界测试：空用户名、特殊字符、集群环境
4. 兼容性测试：Redis 6/7/8、Valkey

## 十、ACL 类别参考

Redis 内置的命令类别：

| 类别           | 说明                              |
| -------------- | --------------------------------- |
| `@all`         | 所有命令                          |
| `@read`        | 只读命令（GET、HGET 等）          |
| `@write`       | 写入命令（SET、DEL 等）           |
| `@admin`       | 管理命令（CONFIG、DEBUG 等）      |
| `@fast`        | 快速命令（O(1) 复杂度）           |
| `@slow`        | 慢速命令                          |
| `@dangerous`   | 危险命令（FLUSHALL、KEYS 等）     |
| `@connection`  | 连接管理（AUTH、SELECT 等）       |
| `@transaction` | 事务命令（MULTI、EXEC 等）        |
| `@scripting`   | 脚本命令（EVAL、SCRIPT 等）       |
| `@pubsub`      | 发布订阅（PUBLISH、SUBSCRIBE 等） |

## 十一、注意事项

1. **版本检测**：连接 Redis 时需检查版本 >= 6.0，否则提示不支持
2. **权限验证**：当前连接用户需具有 ACL 管理权限
3. **default 用户**：删除或禁用 `default` 用户需谨慎
4. **密码安全**：密码在 Redis 中以哈希形式存储，`ACL GETUSER` 返回的是哈希值
5. **集群同步**：集群模式下 ACL 配置会自动广播到所有节点
6. **只读模式**：`share.readonly` 为 true 时隐藏所有编辑按钮
7. **性能考虑**：用户数量较多时，并行获取详情可能导致性能问题，可考虑批量接口优化

## 十二、后续扩展方向

已实现（见 `AclLog.vue`、`AclDryrun.vue`、`UserAdd.vue` 重置密码等）：

- ACL 日志查看
- ACL DRYRUN 模拟执行
- 权限模板预设（`acl.ts`）

仍可扩展：

- ACL 配置导入/导出（文件操作）
- 用户复制/克隆功能
