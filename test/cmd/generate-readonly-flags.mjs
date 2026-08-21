/**
 * 从 Redis 官方 commands.json 生成终端只读判断用的 flags 映射。
 * 运行: node test/cmd/generate-readonly-flags.mjs
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const json = JSON.parse(readFileSync(join(__dirname, 'commands.json'), 'utf8'))

/** 多词命令中视为查询型的子命令动词（CONFIG GET、CLIENT LIST 等） */
const READONLY_VERBS = new Set([
  'GET',
  'HELP',
  'LIST',
  'INFO',
  'COUNT',
  'LEN',
  'SCAN',
  'TYPE',
  'TTL',
  'PTTL',
  'GETNAME',
  'ID',
  'DOCS',
  'WHOAMI',
  'USERS',
  'CAT',
  'LOG',
  'DRYRUN',
  'USAGE',
  'STATS',
  'DOCTOR',
  'GETKEYS',
  'GETKEYSANDFLAGS',
  'GETKEYSINSLOT',
  'GETUSER',
  'KEYSLOT',
  'MYID',
  'NODES',
  'SLOTS',
  'EXISTS',
  'DUMP',
  'OBJECT',
  'RANDOMKEY',
  'SLOT',
  'SHARDS',
  'LINKS',
  'CHANNELS',
  'NUMPAT',
  'NUMSUB',
  'HISTORY',
  'LINDEX',
  'LLEN',
  'LRANGE',
  'LCS',
])

/** 首词命中则整族拒绝（脚本、事务、订阅等） */
const BLOCKLIST_ROOT = new Set([
  'EVAL',
  'EVALSHA',
  'FCALL',
  'MULTI',
  'EXEC',
  'DISCARD',
  'WATCH',
  'UNWATCH',
  'MONITOR',
  'SUBSCRIBE',
  'PSUBSCRIBE',
  'SSUBSCRIBE',
  'UNSUBSCRIBE',
  'PUNSUBSCRIBE',
  'SUNSUBSCRIBE',
  'READWRITE',
  'SHUTDOWN',
  'DEBUG',
  'SLAVEOF',
  'REPLICAOF',
  'FAILOVER',
  'MIGRATE',
  'RESTORE',
  'FUNCTION',
  'SCRIPT',
  'MODULE',
  'SAVE',
  'BGSAVE',
  'BGREWRITEAOF',
  'FLUSHALL',
  'FLUSHDB',
  'SYNC',
  'PSYNC',
  'REPLCONF',
  'WAIT',
  'WAITAOF',
])

/** 单词命令、无 readonly/write 标记但语义只读 */
const READONLY_SINGLES = new Set([
  'INFO',
  'PING',
  'TIME',
  'DBSIZE',
  'LASTSAVE',
  'ROLE',
  'COMMAND',
  'ECHO',
  'SELECT',
  'READONLY',
  'LATENCY',
  'MEMORY',
  'SLOWLOG',
  'PUBSUB',
])

const commandFlags = {}
const grayReadonly = []

for (const [name, def] of Object.entries(json)) {
  const flags = def.command_flags ?? []
  commandFlags[name] = flags

  if (flags.includes('write') || flags.includes('readonly')) continue
  if (BLOCKLIST_ROOT.has(name.split(' ')[0])) continue

  const parts = name.split(' ')
  const verb = parts[parts.length - 1]
  if (parts.length > 1 && READONLY_VERBS.has(verb)) {
    grayReadonly.push(name)
  } else if (parts.length === 1 && READONLY_SINGLES.has(name)) {
    grayReadonly.push(name)
  }
}

// 写入 cmd/index.ts 的标记区，避免覆盖只读判断与 commandHelp 逻辑
const outPath = join(__dirname, '../../src/locales/cmd/index.ts')
const start = '// @generated-cmd-flags-start'
const end = '// @generated-cmd-flags-end'
const generated = `/** 由 test/cmd/generate-readonly-flags.mjs 从 commands.json 生成；8.8/8.10 等手工合并请保留后重新拼接 */
export const commandFlags: Record<string, readonly string[]> = ${JSON.stringify(commandFlags, null, 2)}

export const grayReadonlyCommands: ReadonlySet<string> = new Set(${JSON.stringify(grayReadonly.sort(), null, 2)})
`

const src = readFileSync(outPath, 'utf8')
const startIdx = src.indexOf(start)
const endIdx = src.indexOf(end)
if (startIdx === -1 || endIdx === -1 || endIdx < startIdx) {
  throw new Error(`Missing ${start} / ${end} markers in ${outPath}`)
}
const next = src.slice(0, startIdx + start.length) + '\n' + generated + src.slice(endIdx)
writeFileSync(outPath, next, 'utf8')
console.log(`Updated flags in ${outPath}`)
console.log(`commands: ${Object.keys(commandFlags).length}, gray readonly: ${grayReadonly.length}`)
